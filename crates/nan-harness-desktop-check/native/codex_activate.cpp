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
bool executable(const Request& r,pid_t pid,bool* ancestry_rejected=nullptr) {
    char path[PROC_PIDPATHINFO_MAXSIZE]{}, resolved[PATH_MAX]{};
    if(!alive(r)||proc_pidpath(pid,path,sizeof(path))<=0
        ||!realpath(path,resolved)||r.executable!=resolved)return false;
    bool owned=descendant(r,pid,r.launcher)&&descendant(r,r.launcher,r.root);
    if(!owned&&ancestry_rejected)*ancestry_rejected=true;
    return owned;
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
struct InventoryFailure {
    const char* reason="inventory-unavailable";
    unsigned candidates=0, executable_rejected=0, ancestry_rejected=0;
    unsigned normal_overlap=0, elevated_overlap=0, lower_overlap=0;
    unsigned menu_level=0, status_level=0, dock_level=0, other_elevated=0;
    bool display_contained=false;
};
bool inventory(const Request& r,Binding& result,bool select,InventoryFailure* failure=nullptr,bool activation_only=false,bool* externally_occluded=nullptr) {
    InventoryFailure observation;
    if(!alive(r)){observation.reason="deadline";if(failure)*failure=observation;return false;}
    auto rows=CGWindowListCopyWindowInfo(kCGWindowListOptionOnScreenOnly|kCGWindowListExcludeDesktopElements,kCGNullWindowID);
    if(!rows){if(failure)*failure=observation;return false;}
    CFIndex count=CFArrayGetCount(rows), held=-1;unsigned candidates=0;
    bool valid=count<=1024, other_owned_normal=false, overlapping_ahead=false, owned_overlap=false;
    observation.reason=valid?"metadata":"limit";
    for(CFIndex i=0;valid&&i<count;++i) {
        auto row=(CFDictionaryRef)CFArrayGetValueAtIndex(rows,i);
        if(number(row,kCGWindowLayer)!=0)continue;
        double alpha=0;auto opacity=CFDictionaryGetValue(row,kCGWindowAlpha);
        if(!opacity||CFGetTypeID(opacity)!=CFNumberGetTypeID()
            ||!CFNumberGetValue((CFNumberRef)opacity,kCFNumberDoubleType,&alpha)||!std::isfinite(alpha)){valid=false;break;}
        if(alpha<=0)continue;
        CGRect bounds{};if(!rect(row,bounds)){observation.reason="geometry";valid=false;break;}
        pid_t pid=pid_t(number(row,kCGWindowOwnerPID));
        if(bounds.size.width<300||bounds.size.height<200)continue;
        bool ancestry_rejected=false;
        if(!executable(r,pid,&ancestry_rejected)) {
            if(ancestry_rejected)++observation.ancestry_rejected;
            else ++observation.executable_rejected;
            continue;
        }
        ++candidates;held=i;
        Binding candidate{uint64_t(number(row,kCGWindowNumber)),pid,bounds,0,0};
        ProcessIdentity process{};
        if(!identity(pid,process)){observation.reason="process-identity";valid=false;break;}
        candidate.seconds=process.seconds;candidate.micros=process.micros;
        if(select)result=candidate;
        else if(!same_codex_main(private_identity(result),private_identity(candidate))){observation.reason="identity";valid=false;}
    }
    observation.candidates=candidates;
    if(valid)observation.reason=candidates==0?"candidates-missing":candidates>1?"candidates-ambiguous":"identity";
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
        CGRect bounds{};if(!rect(row,bounds)){observation.reason="geometry";valid=false;break;}
        const bool same_owner=number(row,kCGWindowOwnerPID)==result.pid;
        if(CGRectIntersectsRect(bounds,result.bounds)
            ||(same_owner&&number(row,kCGWindowLayer)==0)) {
            if(CGRectIntersectsRect(bounds,result.bounds)) {
                const auto level=number(row,kCGWindowLayer);
                if(level==0)++observation.normal_overlap;
                else if(level>0) {
                    ++observation.elevated_overlap;
                    if(level==CGWindowLevelForKey(kCGMainMenuWindowLevelKey))++observation.menu_level;
                    else if(level==CGWindowLevelForKey(kCGStatusWindowLevelKey))++observation.status_level;
                    else if(level==CGWindowLevelForKey(kCGDockWindowLevelKey))++observation.dock_level;
                    else ++observation.other_elevated;
                }
                else ++observation.lower_overlap;
            }
            overlapping_ahead=true;
            owned_overlap=owned_overlap||same_owner;
        }
    }
    bool contained=false;
    for(NSScreen* screen in NSScreen.screens) {
        auto id=[screen.deviceDescription[@"NSScreenNumber"] unsignedIntValue];
        contained=contained||CGRectContainsRect(CGDisplayBounds(id),result.bounds);
    }
    observation.display_contained=contained;
    CFRelease(rows);
    bool timely=alive(r);
    bool admitted=codex_inventory_admitted(valid,candidates,held>=0,
        other_owned_normal,overlapping_ahead,contained,activation_only,owned_overlap)&&timely;
    observation.reason=codex_inventory_failure_reason(observation.reason,valid,
        other_owned_normal,overlapping_ahead,contained,timely);
    if(externally_occluded)*externally_occluded=admitted&&overlapping_ahead&&!owned_overlap;
    if(failure)*failure=observation;
    return admitted;
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
static int activation_rejected(const char* boundary,const InventoryFailure* inventory=nullptr) {
    std::cout << "activation-rejected " << boundary;
    if(inventory)std::cout << ' ' << inventory->reason << ' ' << inventory->candidates
        << ' ' << inventory->executable_rejected << ' ' << inventory->ancestry_rejected;
    std::cout << '\n';
    return std::cout ? 5 : 4;
}
#endif
int codex_activate_main() {
#if defined(__APPLE__)
    @autoreleasepool {
        Request request;if(!parse(request))return activation_rejected("request");
        Binding held=request.held;
        InventoryFailure failure;
        // Bringing our sole owned window forward may start behind another app.
        // Verification can observe a pending external stack transition after
        // activation; it never reports success or authorizes input while occluded.
        bool first_occluded=false,second_occluded=false;
        if(!inventory(request,held,!request.action&&!request.verify,&failure,true,&first_occluded))return activation_rejected("cg-inventory-before",&failure);
        const auto first_inventory=failure;
        const char* boundary="ax-main-before";
        AXUIElementRef first=main_window(request,held,&boundary);if(!first)return activation_rejected(boundary);
        Binding fresh=held;bool valid=inventory(request,fresh,false,&failure,true,&second_occluded);
        bool inventory_valid=valid;
        boundary=valid?"ax-main-after":"cg-inventory-after";
        AXUIElementRef second=valid?main_window(request,held,&boundary):nullptr;
        if(valid&&second)boundary="identity";
        valid=valid&&second&&CFEqual(first,second)&&alive(request);
        if(second)CFRelease(second);
        if(!valid){CFRelease(first);return activation_rejected(boundary,inventory_valid?nullptr:&failure);}
        if(request.verify) {
            // Both inventories and AX main identities above are complete and
            // stable. Only an external overlap may settle passively; changes,
            // owned overlaps and query failures still reject before this point.
            if(first_occluded||second_occluded) {
                const auto& stack=second_occluded?failure:first_inventory;
                CFRelease(first);std::cout<<"pending-external-stack "
                    <<(second_occluded?"after":"before")<<' '<<stack.normal_overlap<<' '
                    <<stack.elevated_overlap<<' '<<stack.lower_overlap<<' '
                    <<(stack.display_contained?1:0)<<' '<<stack.menu_level<<' '<<stack.status_level<<' '
                    <<stack.dock_level<<' '<<stack.other_elevated<<'\n';return 0;
            }
            auto app=[NSRunningApplication runningApplicationWithProcessIdentifier:held.pid];
            AXUIElementRef application=AXUIElementCreateApplication(held.pid);CFTypeRef focused=nullptr;
            // Same ordered reads as the boolean proof; no failure retries them.
            const auto rejection=[&]() -> const char* {
                if(!app)return "app-unavailable";
                if(!app.active||app.hidden)return "app-unfocused";
                if([[[NSWorkspace sharedWorkspace] frontmostApplication] processIdentifier]!=held.pid)
                    return "foreground-unfocused";
                if(!application||!attribute(request,application,kAXFocusedWindowAttribute,focused))
                    return "focused-window-query";
                if(CFGetTypeID(focused)!=AXUIElementGetTypeID())return "focused-window-type";
                if(!CFEqual(first,focused))return "focused-window-identity";
                return alive(request)?nullptr:"deadline";
            }();
            if(focused)CFRelease(focused);if(application)CFRelease(application);CFRelease(first);
            if(rejection)return activation_rejected(rejection);std::cout<<"verified\n";return 0;
        }
        if(!request.action) {
            std::cout<<std::setprecision(17)<<held.id<<' '<<held.pid<<' '<<held.bounds.origin.x<<' '
                <<held.bounds.origin.y<<' '<<held.bounds.size.width<<' '<<held.bounds.size.height<<' '
                <<held.seconds<<' '<<held.micros;
            timespec ticks{};if(clock_gettime(CLOCK_MONOTONIC,&ticks)!=0){CFRelease(first);return 5;}
            std::cout<<' '<<(uint64_t(ticks.tv_sec)*1000000000ULL+uint64_t(ticks.tv_nsec))<<'\n';CFRelease(first);return 0;
        }
        auto app=[NSRunningApplication runningApplicationWithProcessIdentifier:held.pid];
        if(!app){CFRelease(first);return activation_rejected("app-unavailable");}
        if(!alive(request)){CFRelease(first);return activation_rejected("deadline");}
        if(![app activateWithOptions:NSApplicationActivateIgnoringOtherApps]){CFRelease(first);return activation_rejected("app-activate");}
        // The first activation is consumed. Any later failure is terminal.
        Binding final=held;
        bool final_inventory=inventory(request,final,false,&failure,true);
        boundary=final_inventory?"ax-main-after":"cg-inventory-after";
        AXUIElementRef current=final_inventory?main_window(request,held,&boundary):nullptr;
        if(current)boundary="identity";
        bool unchanged=current&&CFEqual(first,current);
        if(unchanged){boundary="deadline";unchanged=alive(request);}
        if(current)CFRelease(current);
        if(!unchanged){CFRelease(first);return activation_rejected(boundary,final_inventory?nullptr:&failure);}
        AXError raised=AXUIElementPerformAction(first,kAXRaiseAction);CFRelease(first);
        if(raised!=kAXErrorSuccess)return activation_rejected("raise");
        if(!alive(request))return activation_rejected("deadline");
        std::cout<<"activated\n";return 0;
    }
#else
    return 5;
#endif
}
