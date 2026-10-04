// Scoped native main-window activation. All identities remain private transport.
#include <iostream>
#include <sstream>
#include <string>
#include <cstdint>
#include <cmath>
#include <iomanip>
#include "codex_activation_identity.hpp"
#include "codex_occluder_kind.hpp"
#include "codex_point_stack.hpp"
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
struct Request { bool action=false, verify=false, observe=false; double css_width=0,css_height=0,css_x=0,css_y=0; pid_t caller=0, root=0, launcher=0; uint64_t cutoff=0; std::string executable,document_url; Binding held; };
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
struct OccluderOwner {
    pid_t pid=0; CodexOccluderKind kind=CodexOccluderKind::Unobserved;
    unsigned windows=0, other_public=3; std::vector<ProcessIdentity> chain;
};
bool stable_occluder(const Request& r,const OccluderOwner& owner) {
    if(owner.chain.empty())return false;
    for(const auto& held:owner.chain) {
        ProcessIdentity fresh{};
        if(!alive(r)||!identity(held.pid,fresh)||!same(held,fresh))return false;
    }
    return alive(r);
}
OccluderOwner occluder_owner(const Request& r,pid_t pid) {
    OccluderOwner result;result.pid=pid;
    ProcessIdentity original{};
    char path[PROC_PIDPATHINFO_MAXSIZE]{},resolved[PATH_MAX]{};
    if(!alive(r)||pid<=1||!identity(pid,original))return result;
    const int length=proc_pidpath(pid,path,sizeof(path));
    if(length<=0||unsigned(length)>=sizeof(path)||path[length]!='\0'
        ||!realpath(path,resolved))return result;
    result.other_public=codex_other_public_executable(resolved);
    result.chain.push_back(original);
    // Exact public system paths need only stable process identity. Other owners
    // require a complete stable ancestry chain; query failure is not "other".
    result.kind=codex_occluder_kind(resolved,true,false,false,false);
    if(result.kind==CodexOccluderKind::Unobserved) {
        pid_t next=original.parent;bool complete=next==1;
        for(unsigned depth=1;!complete&&depth<32&&alive(r);++depth) {
            if(next<=1)break;
            bool cycle=false;for(const auto& prior:result.chain)cycle=cycle||prior.pid==next;
            if(cycle)break;
            ProcessIdentity parent{};if(!identity(next,parent))break;
            result.chain.push_back(parent);next=parent.parent;complete=next==1;
        }
        bool launcher=false,checker=false;
        for(const auto& node:result.chain) {
            launcher=launcher||node.pid==r.launcher;checker=checker||node.pid==r.root;
        }
        result.kind=codex_occluder_kind(resolved,true,complete,launcher,checker);
    }
    if(!stable_occluder(r,result))result.kind=CodexOccluderKind::Unobserved;
    return result;
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
    bool display_contained=false, workarea_measured=false, workarea_contained=false;
    unsigned workarea_overlap=0;
    std::array<unsigned,9> occluder_kinds{};
    std::array<unsigned,3> other_public_executables{};
};
// Diagnostic coordinate conversion only; full-display admission is unchanged.
bool workarea(const Request& r,CGRect held,CGRect& usable) {
    NSArray<NSScreen*>* screens=NSScreen.screens;
    if(screens.count==0||screens.count>64||!alive(r))return false;
    const CGRect primary=CGDisplayBounds(CGMainDisplayID());
    if(!geometry(primary)||primary.origin.x!=0||primary.origin.y!=0)return false;
    const CGFloat height=primary.size.height;
    auto quartz=[height](NSRect frame) {
        return CGRectMake(frame.origin.x,height-NSMaxY(frame),frame.size.width,frame.size.height);
    };
    if(!CGRectEqualToRect(quartz(screens[0].frame),primary))return false;
    unsigned matches=0;
    for(NSScreen* screen in screens) {
        NSNumber* number=screen.deviceDescription[@"NSScreenNumber"];
        if(![number isKindOfClass:NSNumber.class])return false;
        const CGRect display=CGDisplayBounds(number.unsignedIntValue);
        if(!CGRectContainsRect(display,held))continue;
        ++matches;
        const CGRect frame=quartz(screen.frame), visible=quartz(screen.visibleFrame);
        if(!geometry(frame)||!geometry(visible)||!CGRectEqualToRect(frame,display)
            ||!CGRectContainsRect(display,visible)||!alive(r))return false;
        usable=visible;
    }
    return matches==1&&alive(r);
}
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
    CGRect usable{};
    if(valid) {
        observation.workarea_measured=workarea(r,result.bounds,usable);
        observation.workarea_contained=observation.workarea_measured&&CGRectContainsRect(usable,result.bounds);
    }
    std::vector<OccluderOwner> owners;
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
                const pid_t owner_pid=pid_t(number(row,kCGWindowOwnerPID));
                OccluderOwner* owner=nullptr;
                for(auto& prior:owners)if(prior.pid==owner_pid){owner=&prior;break;}
                if(!owner&&owners.size()<64) {
                    owners.push_back(occluder_owner(r,owner_pid));owner=&owners.back();
                }
                if(owner)++owner->windows;
                else ++observation.occluder_kinds[unsigned(CodexOccluderKind::Unobserved)];
                if(observation.workarea_measured
                    &&CGRectIntersectsRect(CGRectIntersection(bounds,result.bounds),usable))++observation.workarea_overlap;
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
    for(const auto& owner:owners) {
        const auto kind=stable_occluder(r,owner)?owner.kind:CodexOccluderKind::Unobserved;
        observation.occluder_kinds[unsigned(kind)]+=owner.windows;
        if(kind==CodexOccluderKind::Other&&owner.other_public<3)
            observation.other_public_executables[owner.other_public]+=owner.windows;
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

// Read-only feasibility: geometry is private transport, never input authority.
struct WebAreaObservation { unsigned count=0,nodes=0; AXUIElementRef element=nullptr;
    CGRect bounds{}; bool url_matched=false; const char* reason="measured"; };
bool visit_webareas(const Request& r,AXUIElementRef node,const Binding& held,
    unsigned depth,WebAreaObservation& out) {
    if(!alive(r)||depth>24||++out.nodes>512){out.reason="ax-limit-or-deadline";return false;}
    pid_t pid=0;CFTypeRef role=nullptr;
    if(AXUIElementGetPid(node,&pid)!=kAXErrorSuccess||pid!=held.pid
        ||!attribute(r,node,kAXRoleAttribute,role)){out.reason="ax-query";return false;}
    const bool web=CFEqual(role,CFSTR("AXWebArea"));CFRelease(role);
    if(web) {
        CFTypeRef hidden=nullptr,position=nullptr,size=nullptr;
        bool visible=attribute(r,node,CFSTR("AXHidden"),hidden)
            &&CFGetTypeID(hidden)==CFBooleanGetTypeID()&&!CFBooleanGetValue((CFBooleanRef)hidden);
        if(hidden)CFRelease(hidden);
        if(!visible){out.reason="ax-visibility-unavailable";return false;}
        bool measured=attribute(r,node,kAXPositionAttribute,position)&&attribute(r,node,kAXSizeAttribute,size);
        CGPoint origin{};CGSize dimensions{};
        measured=measured&&CFGetTypeID(position)==AXValueGetTypeID()&&CFGetTypeID(size)==AXValueGetTypeID()
            &&AXValueGetValue((AXValueRef)position,kAXValueTypeCGPoint,&origin)
            &&AXValueGetValue((AXValueRef)size,kAXValueTypeCGSize,&dimensions);
        if(position)CFRelease(position);if(size)CFRelease(size);
        CGRect bounds=CGRectMake(origin.x,origin.y,dimensions.width,dimensions.height);
        if(!measured||!geometry(bounds)||!CGRectContainsRect(held.bounds,bounds)) {
            out.reason="ax-webarea-geometry";return false;
        }
        CFTypeRef url=nullptr;
        CFStringRef expected=CFStringCreateWithBytes(kCFAllocatorDefault,
            (const UInt8*)r.document_url.data(),r.document_url.size(),kCFStringEncodingUTF8,false);
        bool matched=expected&&attribute(r,node,kAXURLAttribute,url)
            &&((CFGetTypeID(url)==CFURLGetTypeID()&&CFEqual(CFURLGetString((CFURLRef)url),expected))
                ||(CFGetTypeID(url)==CFStringGetTypeID()&&CFEqual(url,expected)));
        if(url)CFRelease(url);if(expected)CFRelease(expected);
        ++out.count;out.url_matched=matched;
        if(out.count==1){out.element=(AXUIElementRef)CFRetain(node);out.bounds=bounds;}
        if(out.count>1){out.reason="ax-webarea-ambiguous";return false;}
    }
    CFTypeRef children=nullptr;
    if(AXUIElementSetMessagingTimeout(node,0.1f)!=kAXErrorSuccess){out.reason="ax-query";return false;}
    const AXError error=AXUIElementCopyAttributeValue(node,kAXChildrenAttribute,&children);
    if(error==kAXErrorNoValue||error==kAXErrorAttributeUnsupported)return alive(r);
    if(error!=kAXErrorSuccess||!children||CFGetTypeID(children)!=CFArrayGetTypeID()) {
        if(children)CFRelease(children);out.reason="ax-query";return false;
    }
    auto rows=(CFArrayRef)children;bool valid=CFArrayGetCount(rows)<=512;
    for(CFIndex i=0;valid&&i<CFArrayGetCount(rows);++i) {
        auto child=CFArrayGetValueAtIndex(rows,i);
        valid=CFGetTypeID(child)==AXUIElementGetTypeID()
            &&visit_webareas(r,(AXUIElementRef)child,held,depth+1,out);
    }
    CFRelease(children);if(!valid&&out.reason==std::string("measured"))out.reason="ax-query";
    return valid&&alive(r);
}
bool native_point_clear(const Request& r,const Binding& held,CGPoint point,const char** reason) {
    CFArrayRef rows=CGWindowListCopyWindowInfo(kCGWindowListOptionOnScreenOnly,kCGNullWindowID);
    std::vector<CodexPointStackRow> sample;
    const bool available=rows&&CFArrayGetCount(rows)<=1024;
    if(available)for(CFIndex i=0;i<CFArrayGetCount(rows);++i) {
        CodexPointStackRow item;
        const auto value=CFArrayGetValueAtIndex(rows,i);
        if(CFGetTypeID(value)!=CFDictionaryGetTypeID()){sample.push_back(item);continue;}
        const auto row=(CFDictionaryRef)value;
        auto integer=[row](CFStringRef key,int64_t& out) {
            const auto value=CFDictionaryGetValue(row,key);
            return value&&CFGetTypeID(value)==CFNumberGetTypeID()
                &&CFNumberGetValue((CFNumberRef)value,kCFNumberSInt64Type,&out);
        };
        int64_t id=0,layer=0,pid=0;
        item.identity_valid=integer(kCGWindowNumber,id)&&id>0&&integer(kCGWindowLayer,layer);
        item.held=uint64_t(id)==held.id;
        item.cursor=layer==CGWindowLevelForKey(kCGCursorWindowLevelKey);
        CGRect bounds{};
        item.bounds_valid=rect(row,bounds);
        item.point_contained=item.bounds_valid&&CGRectContainsPoint(bounds,point);
        item.held_metadata_valid=integer(kCGWindowOwnerPID,pid)&&pid>1&&item.bounds_valid;
        item.held_unchanged=item.held_metadata_valid&&pid==held.pid&&layer==0
            &&CGRectEqualToRect(bounds,held.bounds);
        double alpha=0;const auto opacity=CFDictionaryGetValue(row,kCGWindowAlpha);
        item.alpha_valid=opacity&&CFGetTypeID(opacity)==CFNumberGetTypeID()
            &&CFNumberGetValue((CFNumberRef)opacity,kCFNumberDoubleType,&alpha)&&std::isfinite(alpha);
        item.transparent=item.alpha_valid&&alpha<=0;
        sample.push_back(item);
    }
    if(rows)CFRelease(rows);
    const auto result=codex_point_stack_result(available,sample);
    *reason=codex_point_stack_reason(result);
    return result==CodexPointStackResult::Clear&&alive(r);
}
bool native_hit_window(const Request& r,const Binding& held,AXUIElementRef main,CGPoint point) {
    if(!alive(r))return false;
    AXUIElementRef system=AXUIElementCreateSystemWide(),hit=nullptr;
    bool matched=false;
    if(system&&AXUIElementSetMessagingTimeout(system,0.1f)==kAXErrorSuccess
        &&AXUIElementCopyElementAtPosition(system,point.x,point.y,&hit)==kAXErrorSuccess&&hit) {
        pid_t pid=0;CFTypeRef window=nullptr;
        const bool owner=AXUIElementGetPid(hit,&pid)==kAXErrorSuccess&&pid==held.pid;
        const bool self=CFEqual(hit,main);
        const bool enclosing=owner&&!self&&attribute(r,hit,kAXWindowAttribute,window)
            &&CFGetTypeID(window)==AXUIElementGetTypeID()&&CFEqual(window,main);
        matched=codex_hit_is_held_window(owner,self,enclosing)&&alive(r);
        if(window)CFRelease(window);
    }
    if(hit)CFRelease(hit);if(system)CFRelease(system);
    return matched&&alive(r);
}
int point_observe(const Request& r,const Binding& held,AXUIElementRef main) {
    const char* reason="measured";WebAreaObservation first,second;
    bool measured=visit_webareas(r,main,held,0,first);
    if(!measured)reason=first.reason;
    else if(first.count!=1)reason="ax-webarea-missing";
    else if(!visit_webareas(r,main,held,0,second))reason=second.reason;
    else if(second.count!=1||!CFEqual(first.element,second.element)
        ||!CGRectEqualToRect(first.bounds,second.bounds))reason="ax-webarea-changed";
    const char* point_reason="native-point-not-clear";
    bool stable=std::string(reason)=="measured",url_matched=stable&&first.url_matched&&second.url_matched,focus=false,dimensions=false,clear=false,hit_owned=false;
    if(stable) {
        auto app=[NSRunningApplication runningApplicationWithProcessIdentifier:held.pid];
        AXUIElementRef application=AXUIElementCreateApplication(held.pid);CFTypeRef focused=nullptr;
        focus=app&&app.active&&!app.hidden
            &&[[[NSWorkspace sharedWorkspace] frontmostApplication] processIdentifier]==held.pid
            &&application&&attribute(r,application,kAXFocusedWindowAttribute,focused)
            &&CFGetTypeID(focused)==AXUIElementGetTypeID()&&CFEqual(main,focused);
        if(focused)CFRelease(focused);if(application)CFRelease(application);
        dimensions=first.bounds.size.width==r.css_width&&first.bounds.size.height==r.css_height;
        if(focus&&dimensions) {
            CGPoint point=CGPointMake(first.bounds.origin.x+r.css_x,first.bounds.origin.y+r.css_y);
            clear=native_point_clear(r,held,point,&point_reason);
            hit_owned=native_hit_window(r,held,main,point);
        }
        if(!focus)reason="native-focus-unavailable";
        else if(!dimensions)reason="viewport-dimensions-mismatch";
        else if(!clear)reason=point_reason;
        else if(!hit_owned)reason="native-hit-window-unproved";
        else reason=url_matched?"mapping-observed":"renderer-webarea-correlation-unproved";
    }
    if(stable) {
        WebAreaObservation final_area;
        const bool area_ok=visit_webareas(r,main,held,0,final_area)&&final_area.count==1
            &&CFEqual(first.element,final_area.element)&&CGRectEqualToRect(first.bounds,final_area.bounds)
            &&first.url_matched==final_area.url_matched;
        second.count=final_area.count;
        if(final_area.element)CFRelease(final_area.element);
        if(!area_ok) {
            reason="ax-webarea-changed";stable=false;focus=false;dimensions=false;
            clear=false;hit_owned=false;url_matched=false;
        } else {
            auto app=[NSRunningApplication runningApplicationWithProcessIdentifier:held.pid];
            AXUIElementRef application=AXUIElementCreateApplication(held.pid);CFTypeRef focused=nullptr;
            const bool final_focus=app&&app.active&&!app.hidden
                &&[[[NSWorkspace sharedWorkspace] frontmostApplication] processIdentifier]==held.pid
                &&application&&attribute(r,application,kAXFocusedWindowAttribute,focused)
                &&CFGetTypeID(focused)==AXUIElementGetTypeID()&&CFEqual(main,focused);
            if(focused)CFRelease(focused);if(application)CFRelease(application);
            if(focus&&!final_focus){focus=false;reason="native-focus-unavailable";}
            if(clear&&!native_point_clear(r,held,CGPointMake(first.bounds.origin.x+r.css_x,
                first.bounds.origin.y+r.css_y),&point_reason)){clear=false;if(focus)reason=point_reason;}
            if(hit_owned&&!native_hit_window(r,held,main,CGPointMake(first.bounds.origin.x+r.css_x,
                first.bounds.origin.y+r.css_y))){hit_owned=false;if(focus&&clear)reason="native-hit-window-unproved";}
        }
    }
    // Recheck immutable identity after every read. No replacement/renewal/activation.
    Binding fresh=held;InventoryFailure failure;
    const bool inventory_ok=inventory(r,fresh,false,&failure,true);
    AXUIElementRef current=inventory_ok?main_window(r,held):nullptr;
    const bool held_stable=current&&CFEqual(main,current)&&alive(r);
    if(current)CFRelease(current);if(!held_stable)reason="held-identity-or-deadline";
    std::cout<<std::setprecision(17)<<"point-observation "<<reason<<' '
        <<first.count<<' '<<second.count<<' '<<stable<<' '<<focus<<' '<<dimensions<<' '
        <<clear<<' '<<hit_owned<<' '<<held_stable<<' '<<url_matched;
    if(stable)std::cout<<' '<<first.bounds.origin.x<<' '<<first.bounds.origin.y<<' '
        <<first.bounds.size.width<<' '<<first.bounds.size.height;
    std::cout<<'\n';
    if(first.element)CFRelease(first.element);if(second.element)CFRelease(second.element);
    return std::cout?0:4;
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
    r.caller=pid_t(caller);r.root=pid_t(root);r.launcher=pid_t(launcher);r.action=phase=="activate";r.verify=phase=="verify";r.observe=phase=="point-observe";
    if(phase!="prepare"&&!r.action&&!r.verify&&!r.observe)return false;
    for(size_t i=0;i<hex.size();i+=2) {
        auto digit=[](char c)->int{return c>='0'&&c<='9'?c-'0':c>='a'&&c<='f'?c-'a'+10:-1;};
        int a=digit(hex[i]),b=digit(hex[i+1]);if(a<0||b<0||!(a*16+b))return false;
        r.executable.push_back(char(a*16+b));
    }
    if(r.executable[0]!='/')return false;
    if(r.action||r.verify||r.observe) {
        if(!(input>>r.held.id>>pid>>r.held.bounds.origin.x>>r.held.bounds.origin.y
            >>r.held.bounds.size.width>>r.held.bounds.size.height>>r.held.seconds>>r.held.micros)
            ||!r.held.id||pid<=1||pid>INT_MAX||!r.held.seconds||!geometry(r.held.bounds))return false;
        r.held.pid=pid_t(pid);
    }
    if(r.observe&&(!(input>>r.css_width>>r.css_height>>r.css_x>>r.css_y)
        ||!std::isfinite(r.css_width)||!std::isfinite(r.css_height)
        ||!std::isfinite(r.css_x)||!std::isfinite(r.css_y)
        ||r.css_width<1||r.css_height<1||r.css_width>16384||r.css_height>16384
        ||r.css_x<=0||r.css_y<=0||r.css_x>=r.css_width||r.css_y>=r.css_height))return false;
    if(r.observe) {
        std::string urlhex;if(!(input>>urlhex)||urlhex.empty()||urlhex.size()>16384||urlhex.size()%2)return false;
        for(size_t i=0;i<urlhex.size();i+=2) {
            auto digit=[](char c)->int{return c>='0'&&c<='9'?c-'0':c>='a'&&c<='f'?c-'a'+10:-1;};
            int a=digit(urlhex[i]),b=digit(urlhex[i+1]);if(a<0||b<0||!(a*16+b))return false;
            r.document_url.push_back(char(a*16+b));
        }
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
        if(!inventory(request,held,!request.action&&!request.verify&&!request.observe,&failure,true,&first_occluded))return activation_rejected("cg-inventory-before",&failure);
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
        if(request.observe){int result=point_observe(request,held,first);CFRelease(first);return result;}
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
                    <<stack.dock_level<<' '<<stack.other_elevated<<' '
                    <<(stack.workarea_measured?1:0)<<' '<<(stack.workarea_contained?1:0)<<' '
                    <<stack.workarea_overlap;
                for(const auto value:stack.occluder_kinds)std::cout<<' '<<value;
                for(const auto value:stack.other_public_executables)std::cout<<' '<<value;
                std::cout<<'\n';return 0;
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
