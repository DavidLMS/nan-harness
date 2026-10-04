// Scoped native main-window activation. All identities remain private transport.
#include <iostream>
#include <sstream>
#include <string>
#include <cstdint>
#include <cmath>
#include <iomanip>
#include "codex_activation_identity.hpp"
#if defined(__APPLE__)
#import <AppKit/AppKit.h>
#include <ApplicationServices/ApplicationServices.h>
#include <libproc.h>
#include <unistd.h>
#include <time.h>
#include <vector>
#include <limits.h>
namespace {
struct ProcessIdentity { pid_t pid, parent; uint64_t seconds, micros; };
struct Binding { uint64_t id=0; pid_t pid=0; CGRect bounds{}; uint64_t seconds=0, micros=0; };
CodexMainIdentity private_identity(const Binding& value) {
    return {value.id,uint64_t(value.pid),value.seconds,value.micros,value.bounds.origin.x,
        value.bounds.origin.y,value.bounds.size.width,value.bounds.size.height};
}
struct Request { bool action=false, verify=false; pid_t caller=0, root=0, launcher=0; uint64_t cutoff=0; std::string executable; Binding held; };
bool alive(const Request& r) {
    timespec now{};
    return getppid()==r.caller && clock_gettime(CLOCK_MONOTONIC,&now)==0
        && now.tv_sec>=0 && uint64_t(now.tv_sec)*1000000000ULL+uint64_t(now.tv_nsec)<r.cutoff;
}
bool identity(pid_t pid,ProcessIdentity& result) {
    proc_bsdinfo info{};
    if(proc_pidinfo(pid,PROC_PIDTBSDINFO,0,&info,sizeof(info))!=sizeof(info)
        || info.pbi_pid!=uint32_t(pid) || !info.pbi_start_tvsec) return false;
    result={pid,pid_t(info.pbi_ppid),info.pbi_start_tvsec,info.pbi_start_tvusec};return true;
}
bool same(const ProcessIdentity& a,const ProcessIdentity& b) {
    return a.pid==b.pid&&a.parent==b.parent&&a.seconds==b.seconds&&a.micros==b.micros;
}
bool descendant(const Request& r,pid_t child,pid_t ancestor) {
    std::vector<ProcessIdentity> chain;
    for(unsigned depth=0;depth<32&&alive(r);++depth) {
        ProcessIdentity value{};
        if(child<=1||!identity(child,value))return false;
        for(const auto& prior:chain)if(prior.pid==child)return false;
        chain.push_back(value);
        if(child==ancestor) {
            for(const auto& prior:chain) {
                ProcessIdentity fresh{};
                if(!alive(r)||!identity(prior.pid,fresh)||!same(prior,fresh))return false;
            }
            return alive(r);
        }
        child=value.parent;
    }
    return false;
}
bool executable(const Request& r,pid_t pid) {
    char path[PROC_PIDPATHINFO_MAXSIZE]{}, resolved[PATH_MAX]{};
    return alive(r)&&proc_pidpath(pid,path,sizeof(path))>0
        && realpath(path,resolved)&&r.executable==resolved
        && descendant(r,pid,r.launcher)&&descendant(r,r.launcher,r.root);
}
int64_t number(CFDictionaryRef row,CFStringRef key) {
    auto value=CFDictionaryGetValue(row,key);int64_t result=0;
    if(value&&CFGetTypeID(value)==CFNumberGetTypeID())CFNumberGetValue((CFNumberRef)value,kCFNumberSInt64Type,&result);
    return result;
}
bool geometry(CGRect rect) {
    return std::isfinite(rect.origin.x)&&std::isfinite(rect.origin.y)
        &&std::isfinite(rect.size.width)&&std::isfinite(rect.size.height)
        &&rect.size.width>0&&rect.size.height>0;
}
bool rect(CFDictionaryRef row,CGRect& result) {
    auto value=CFDictionaryGetValue(row,kCGWindowBounds);
    return value&&CFGetTypeID(value)==CFDictionaryGetTypeID()
        &&CGRectMakeWithDictionaryRepresentation((CFDictionaryRef)value,&result)&&geometry(result);
}
bool inventory(const Request& r,Binding& result,bool select) {
    if(!alive(r))return false;
    auto rows=CGWindowListCopyWindowInfo(kCGWindowListOptionOnScreenOnly|kCGWindowListExcludeDesktopElements,kCGNullWindowID);
    if(!rows)return false;
    CFIndex count=CFArrayGetCount(rows), held=-1;unsigned candidates=0;
    bool valid=count<=1024, other_owned_normal=false, overlapping_ahead=false;
    for(CFIndex i=0;valid&&i<count;++i) {
        auto row=(CFDictionaryRef)CFArrayGetValueAtIndex(rows,i);
        if(number(row,kCGWindowLayer)!=0)continue;
        double alpha=0;auto opacity=CFDictionaryGetValue(row,kCGWindowAlpha);
        if(!opacity||CFGetTypeID(opacity)!=CFNumberGetTypeID()
            ||!CFNumberGetValue((CFNumberRef)opacity,kCFNumberDoubleType,&alpha)||!std::isfinite(alpha)){valid=false;break;}
        if(alpha<=0)continue;
        CGRect bounds{};if(!rect(row,bounds)){valid=false;break;}
        pid_t pid=pid_t(number(row,kCGWindowOwnerPID));
        if(bounds.size.width<300||bounds.size.height<200)continue;
        if(!executable(r,pid))continue;
        ++candidates;held=i;
        Binding candidate{uint64_t(number(row,kCGWindowNumber)),pid,bounds,0,0};
        ProcessIdentity process{};
        if(!identity(pid,process)){valid=false;break;}
        candidate.seconds=process.seconds;candidate.micros=process.micros;
        if(select)result=candidate;
        else if(!same_codex_main(private_identity(result),private_identity(candidate)))valid=false;
    }
    valid=valid&&candidates==1&&held>=0&&result.id!=0;
    for(CFIndex i=0;valid&&i<count;++i) {
        auto row=(CFDictionaryRef)CFArrayGetValueAtIndex(rows,i);
        if(i!=held&&number(row,kCGWindowLayer)==0
            &&(number(row,kCGWindowOwnerPID)==result.pid
                ||descendant(r,pid_t(number(row,kCGWindowOwnerPID)),r.launcher)))other_owned_normal=true;
    }
    for(CFIndex i=0;valid&&i<held;++i) {
        auto row=(CFDictionaryRef)CFArrayGetValueAtIndex(rows,i);
        if(number(row,kCGWindowLayer)==CGWindowLevelForKey(kCGCursorWindowLevelKey))continue;
        double alpha=1;auto value=CFDictionaryGetValue(row,kCGWindowAlpha);
        if(value&&CFGetTypeID(value)==CFNumberGetTypeID())CFNumberGetValue((CFNumberRef)value,kCFNumberDoubleType,&alpha);
        if(alpha<=0)continue;
        CGRect bounds{};if(!rect(row,bounds)){valid=false;break;}
        if(CGRectIntersectsRect(bounds,result.bounds)
            ||(number(row,kCGWindowOwnerPID)==result.pid&&number(row,kCGWindowLayer)==0))overlapping_ahead=true;
    }
    bool contained=false;
    for(NSScreen* screen in NSScreen.screens) {
        auto id=[screen.deviceDescription[@"NSScreenNumber"] unsignedIntValue];
        contained=contained||CGRectContainsRect(CGDisplayBounds(id),result.bounds);
    }
    CFRelease(rows);return codex_inventory_admitted(valid,candidates,held>=0,
        other_owned_normal,overlapping_ahead,contained)&&alive(r);
}
bool attribute(const Request& r,AXUIElementRef element,CFStringRef name,CFTypeRef& value) {
    return alive(r)&&AXUIElementSetMessagingTimeout(element,0.1f)==kAXErrorSuccess
        &&AXUIElementCopyAttributeValue(element,name,&value)==kAXErrorSuccess&&value&&alive(r);
}
AXUIElementRef main_window(const Request& r,const Binding& held,const char** boundary=nullptr) {
    if(!AXIsProcessTrusted()) { if(boundary)*boundary="trust";return nullptr; }
    if(!executable(r,held.pid))return nullptr;
    AXUIElementRef app=AXUIElementCreateApplication(held.pid);CFTypeRef main=nullptr;
    if(!app)return nullptr;
    bool valid=attribute(r,app,kAXMainWindowAttribute,main);CFRelease(app);
    if(!valid||CFGetTypeID(main)!=AXUIElementGetTypeID()){if(main)CFRelease(main);return nullptr;}
    auto window=(AXUIElementRef)main;pid_t pid=0;
    CFTypeRef role=nullptr,subrole=nullptr,position=nullptr,size=nullptr;
    valid=AXUIElementGetPid(window,&pid)==kAXErrorSuccess&&pid==held.pid
        &&attribute(r,window,kAXRoleAttribute,role)&&CFEqual(role,kAXWindowRole)
        &&attribute(r,window,kAXSubroleAttribute,subrole)&&CFEqual(subrole,kAXStandardWindowSubrole)
        &&attribute(r,window,kAXPositionAttribute,position)&&attribute(r,window,kAXSizeAttribute,size);
    CGPoint origin{};CGSize dimensions{};
    valid=valid&&CFGetTypeID(position)==AXValueGetTypeID()&&CFGetTypeID(size)==AXValueGetTypeID()
        &&AXValueGetValue((AXValueRef)position,kAXValueTypeCGPoint,&origin)
        &&AXValueGetValue((AXValueRef)size,kAXValueTypeCGSize,&dimensions)
        &&CGRectEqualToRect(CGRectMake(origin.x,origin.y,dimensions.width,dimensions.height),held.bounds);
    for(auto value:{role,subrole,position,size})if(value)CFRelease(value);
    if(!valid){CFRelease(main);return nullptr;}return window;
}
bool parse(Request& r) {
    std::string line,phase,hex,extra;unsigned long caller=0,root=0,launcher=0,pid=0;
    char buffer[16385]{};
    if(!std::cin.getline(buffer,sizeof(buffer))||std::cin.peek()!=std::char_traits<char>::eof())return false;
    line=buffer;
    std::istringstream input(line);
    if(!(input>>phase>>caller>>root>>launcher>>r.cutoff>>hex)||!r.cutoff
        ||caller<=1||root<=1||launcher<=1||caller>INT_MAX||root>INT_MAX||launcher>INT_MAX
        ||hex.empty()||hex.size()>PATH_MAX*2||hex.size()%2)return false;
    r.caller=pid_t(caller);r.root=pid_t(root);r.launcher=pid_t(launcher);r.action=phase=="activate";r.verify=phase=="verify";
    if(phase!="prepare"&&!r.action&&!r.verify)return false;
    for(size_t i=0;i<hex.size();i+=2) {
        auto digit=[](char c)->int{return c>='0'&&c<='9'?c-'0':c>='a'&&c<='f'?c-'a'+10:-1;};
        int a=digit(hex[i]),b=digit(hex[i+1]);if(a<0||b<0||!(a*16+b))return false;
        r.executable.push_back(char(a*16+b));
    }
    if(r.executable[0]!='/')return false;
    if(r.action||r.verify) {
        if(!(input>>r.held.id>>pid>>r.held.bounds.origin.x>>r.held.bounds.origin.y
            >>r.held.bounds.size.width>>r.held.bounds.size.height>>r.held.seconds>>r.held.micros)
            ||!r.held.id||pid<=1||pid>INT_MAX||!r.held.seconds||!geometry(r.held.bounds))return false;
        r.held.pid=pid_t(pid);
    }
    return !(input>>extra)&&alive(r);
}
}
#endif
#if defined(__APPLE__)
static int activation_rejected(const char* boundary) {
    std::cout << "activation-rejected " << boundary << '\n';
    return std::cout ? 5 : 4;
}
#endif
int codex_activate_main() {
#if defined(__APPLE__)
    @autoreleasepool {
        Request request;if(!parse(request))return activation_rejected("request");
        Binding held=request.held;
        if(!inventory(request,held,!request.action&&!request.verify))return activation_rejected("cg-inventory-before");
        const char* boundary="ax-main-before";
        AXUIElementRef first=main_window(request,held,&boundary);if(!first)return activation_rejected(boundary);
        Binding fresh=held;bool valid=inventory(request,fresh,false);
        boundary=valid?"ax-main-after":"cg-inventory-after";
        AXUIElementRef second=valid?main_window(request,held,&boundary):nullptr;
        if(valid&&second)boundary="identity";
        valid=valid&&second&&CFEqual(first,second)&&alive(request);
        if(second)CFRelease(second);
        if(!valid){CFRelease(first);return activation_rejected(boundary);}
        if(request.verify) {
            auto app=[NSRunningApplication runningApplicationWithProcessIdentifier:held.pid];
            AXUIElementRef application=AXUIElementCreateApplication(held.pid);CFTypeRef focused=nullptr;
            bool proved=app&&app.active&&!app.hidden
                &&[[[NSWorkspace sharedWorkspace] frontmostApplication] processIdentifier]==held.pid
                &&application&&attribute(request,application,kAXFocusedWindowAttribute,focused)
                &&CFGetTypeID(focused)==AXUIElementGetTypeID()&&CFEqual(first,focused)&&alive(request);
            if(focused)CFRelease(focused);if(application)CFRelease(application);CFRelease(first);
            if(!proved)return 5;std::cout<<"verified\n";return 0;
        }
        if(!request.action) {
            std::cout<<std::setprecision(17)<<held.id<<' '<<held.pid<<' '<<held.bounds.origin.x<<' '
                <<held.bounds.origin.y<<' '<<held.bounds.size.width<<' '<<held.bounds.size.height<<' '
                <<held.seconds<<' '<<held.micros;
            timespec ticks{};if(clock_gettime(CLOCK_MONOTONIC,&ticks)!=0){CFRelease(first);return 5;}
            std::cout<<' '<<(uint64_t(ticks.tv_sec)*1000000000ULL+uint64_t(ticks.tv_nsec))<<'\n';CFRelease(first);return 0;
        }
        auto app=[NSRunningApplication runningApplicationWithProcessIdentifier:held.pid];
        if(!app||!alive(request)||![app activateWithOptions:NSApplicationActivateIgnoringOtherApps]){CFRelease(first);return 5;}
        // The first activation is consumed. Any later failure is terminal.
        Binding final=held;
        AXUIElementRef current=inventory(request,final,false)?main_window(request,held):nullptr;
        bool unchanged=current&&CFEqual(first,current)&&alive(request);
        if(current)CFRelease(current);
        if(!unchanged){CFRelease(first);return 5;}
        AXError raised=AXUIElementPerformAction(first,kAXRaiseAction);CFRelease(first);
        if(raised!=kAXErrorSuccess||!alive(request))return 5;
        std::cout<<"activated\n";return 0;
    }
#else
    return 5;
#endif
}
