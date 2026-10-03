#include "uia_request_frame.hpp"
#include "uia_process_identity.hpp"
#include "uia_process_ancestry.hpp"

// Passive source-labelled UIA counts from one freshly guarded owned window.
#ifdef _WIN32
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#include <tlhelp32.h>
#include <UIAutomation.h>
#include <dwmapi.h>
#include <wrl/client.h>
#include <chrono>
#include <cstdint>
#include <iostream>
#include <sstream>
#include <string>
#include <vector>
using Microsoft::WRL::ComPtr;
namespace {
struct Request { HWND window{}; DWORD pid{}; RECT bounds{}; unsigned budget{}; };
using Clock = std::chrono::steady_clock;
struct ProcessHandle {
    HANDLE value = nullptr;
    explicit ProcessHandle(HANDLE handle = nullptr) : value(handle) {}
    ProcessHandle(const ProcessHandle&) = delete;
    ProcessHandle& operator=(const ProcessHandle&) = delete;
    ~ProcessHandle() { if (value && value != INVALID_HANDLE_VALUE) CloseHandle(value); }
};
bool live_creation(HANDLE process, std::uint64_t& time) {
    if (!process || process == INVALID_HANDLE_VALUE || WaitForSingleObject(process, 0) != WAIT_TIMEOUT)
        return false;
    FILETIME created{}, exited{}, kernel{}, user{};
    if (!GetProcessTimes(process, &created, &exited, &kernel, &user)) return false;
    time = (static_cast<std::uint64_t>(created.dwHighDateTime) << 32) | created.dwLowDateTime;
    return time != 0 && WaitForSingleObject(process, 0) == WAIT_TIMEOUT;
}
struct Collection {
    Request request;
    Clock::time_point deadline;
    ComPtr<IUIAutomation> automation;
    ComPtr<IUIAutomationTreeWalker> walker;
    std::vector<ComPtr<IUIAutomationElement>> held;
    unsigned classic{}, modern{}, sends{}, starts{}, headings{}, copies{}, nodes{};
    const char* stage = "query";
    ProcessHandle root_process;
    std::uint64_t root_creation{};
    bool within() const { return Clock::now() < deadline; }
    static bool contains(const RECT& a, const RECT& b) {
        return b.right>b.left && b.bottom>b.top && a.left<=b.left && a.top<=b.top && a.right>=b.right && a.bottom>=b.bottom;
    }
    bool guard() {
        DWORD actual=0, cloak=0; RECT bounds{}; MONITORINFO monitor{sizeof(MONITORINFO)};
        if (!within()) { stage="deadline"; return false; }
        if (!IsWindow(request.window) || !GetWindowThreadProcessId(request.window,&actual) || actual!=request.pid) {stage="identity";return false;}
        if (GetForegroundWindow()!=request.window) {stage="foreground";return false;}
        if (!GetWindowRect(request.window,&bounds) || !EqualRect(&bounds,&request.bounds)) {stage="bounds";return false;}
        if (!IsWindowVisible(request.window) || IsIconic(request.window)
            || FAILED(DwmGetWindowAttribute(request.window,DWMWA_CLOAKED,&cloak,sizeof(cloak))) || cloak) {stage="visibility";return false;}
        HMONITOR m=MonitorFromWindow(request.window,MONITOR_DEFAULTTONULL);
        if (!m || !GetMonitorInfoW(m,&monitor) || !contains(monitor.rcMonitor,bounds)) {stage="display";return false;}
        unsigned count=0;
        for (HWND above=GetWindow(request.window,GW_HWNDPREV);above;above=GetWindow(above,GW_HWNDPREV)) {
            if (!within()) {stage="deadline";return false;}
            if (++count>1024) {stage="limit";return false;}
            if (!IsWindowVisible(above) || IsIconic(above)) continue;
            DWORD c=0,pid=0;RECT front{},intersection{};
            if (FAILED(DwmGetWindowAttribute(above,DWMWA_CLOAKED,&c,sizeof(c)))) {stage="query";return false;}
            if(c) continue;
            if(!GetWindowThreadProcessId(above,&pid) || !GetWindowRect(above,&front)) {stage="query";return false;}
            if(pid==request.pid || IntersectRect(&intersection,&front,&bounds)) {stage="occlusion";return false;}
        }
        return within();
    }
    bool process_snapshot(std::vector<CorrelationEntry>& rows) const {
        if (!within()) return false;
        ProcessHandle snapshot(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0));
        if (snapshot.value == INVALID_HANDLE_VALUE) return false;
        PROCESSENTRY32W row{}; row.dwSize = sizeof(row);
        if (!Process32FirstW(snapshot.value, &row)) return false;
        bool root_present = false;
        do {
            if (!within() || rows.size() >= 4096) return false;
            root_present = root_present || row.th32ProcessID == request.pid;
            rows.push_back({row.th32ProcessID, row.th32ParentProcessID, false});
        } while (Process32NextW(snapshot.value, &row));
        return GetLastError() == ERROR_NO_MORE_FILES && root_present && within();
    }
    const char* mismatched_descendant(DWORD child) {
        constexpr auto unavailable = "descendant-correlation-unavailable";
        std::uint64_t before = 0;
        if (!within() || !live_creation(root_process.value, before) || before != root_creation)
            return unavailable;
        std::vector<CorrelationEntry> rows, confirm;
        if (!process_snapshot(rows) || !process_snapshot(confirm)) return unavailable;
        const auto query = [&](std::uint32_t pid, std::uint64_t& time) {
            if (!within()) return false;
            ProcessHandle process(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, FALSE, pid));
            return live_creation(process.value, time) && within();
        };
        std::uint64_t child_creation = 0;
        if (!query(child, child_creation)) return unavailable;
        const auto relation = classify_uia_ancestry(rows, confirm, child, request.pid,
            root_creation, child_creation, query);
        std::uint64_t after = 0;
        if (!within() || !live_creation(root_process.value, after) || after != root_creation
            || !guard()) return unavailable;
        switch (relation) {
            case UiaAncestry::Owned: return "owned-descendant-process";
            case UiaAncestry::Foreign: return "foreign-descendant-process";
            case UiaAncestry::Unavailable: return unavailable;
        }
        return unavailable;
    }
    bool append(IUIAutomationElement* element,unsigned depth) {
        if(!within()) {stage="deadline";return false;}
        if(depth>32 || ++nodes>1024) {stage="limit";return false;}
        for(const auto& prior:held) {
            BOOL same=FALSE;
            if(FAILED(automation->CompareElements(prior.Get(),element,&same))) return false;
            if(same) {stage="duplicate";return false;}
        }
        int pid=0,type=0; BSTR name=nullptr;
        if(FAILED(element->get_CurrentProcessId(&pid))) {
            stage=depth==0?"root-process-query":"descendant-process-query";return false;
        }
        if(const char* rejected=uia_process_identity_failure(pid,request.pid,depth==0)) {
            stage=depth>0 && pid>0 && static_cast<DWORD>(pid)!=request.pid
                ? mismatched_descendant(static_cast<DWORD>(pid)) : rejected;
            return false;
        }
        if(FAILED(element->get_CurrentControlType(&type)) || FAILED(element->get_CurrentName(&name))) return false;
        std::wstring text;
        if(name) {
            unsigned length=SysStringLen(name);
            if(length>65536) {SecureZeroMemory(name,length*sizeof(wchar_t));SysFreeString(name);stage="limit";return false;}
            text.assign(name,length);SecureZeroMemory(name,length*sizeof(wchar_t));SysFreeString(name);
        }
        struct Wipe {std::wstring& text;~Wipe(){if(!text.empty())SecureZeroMemory(text.data(),text.size()*sizeof(wchar_t));}} wipe{text};
        BOOL offscreen=TRUE,enabled=FALSE;
        if(type==UIA_EditControlTypeId || type==UIA_ButtonControlTypeId) {
            if(FAILED(element->get_CurrentIsOffscreen(&offscreen)) || FAILED(element->get_CurrentIsEnabled(&enabled))) return false;
        }
        if(type==UIA_EditControlTypeId && !offscreen && enabled) {
            classic+=text==L"Write your prompt to Claude";modern+=text==L"Message";
        }
        if(type==UIA_ButtonControlTypeId && !offscreen) {
            sends+=text==L"Send message";starts+=text==L"Start task";copies+=text==L"Copy";
        }
        if(type==UIA_TextControlTypeId) {
            VARIANT heading;VariantInit(&heading);
            HRESULT result=element->GetCurrentPropertyValue(UIA_HeadingLevelPropertyId,&heading);
            bool valid=SUCCEEDED(result) && heading.vt==VT_I4;
            if(valid && heading.lVal>=HeadingLevel1 && heading.lVal<=HeadingLevel9 && text.rfind(L"Claude responded:",0)==0) ++headings;
            VariantClear(&heading);
            if(!valid) {stage="heading-property";return false;}
        }
        ComPtr<IUIAutomationElement> retained=element;held.push_back(retained);
        ComPtr<IUIAutomationElement> child;
        if(FAILED(walker->GetFirstChildElement(element,&child))) return false;
        while(child) {
            if(!append(child.Get(),depth+1)) return false;
            ComPtr<IUIAutomationElement> next;
            if(FAILED(walker->GetNextSiblingElement(child.Get(),&next))) return false;
            child=next;
        }
        return within();
    }
};
}
int windows_claude_uia_inventory() {
    const auto wire = uia_request_frame(std::cin);
    if (!wire) return 2;
    Request r;std::uint64_t id=0;std::string extra;std::istringstream parser(*wire);
    if(!(parser>>id>>r.pid>>r.bounds.left>>r.bounds.top>>r.bounds.right>>r.bounds.bottom>>r.budget)
        || parser>>extra || !id || id>UINTPTR_MAX || !r.pid || !r.budget || r.budget>3000) return 2;
    r.window=reinterpret_cast<HWND>(static_cast<std::uintptr_t>(id));
    Collection collection;collection.request=r;collection.deadline=Clock::now()+std::chrono::milliseconds(r.budget);
    collection.root_process.value = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, FALSE, r.pid);
    live_creation(collection.root_process.value, collection.root_creation);
    const auto emit=[&](bool complete) {
        std::cout<<"uia "<<(complete?"observed":collection.stage)<<' ';
        if(complete) std::cout<<collection.nodes<<' '<<collection.classic<<' '<<collection.modern<<' '<<collection.sends<<' '<<collection.starts<<' '<<collection.headings<<' '<<collection.copies;
        else std::cout<<"- - - - - - -";
        std::cout<<'\n';return std::cout?0:4;
    };
    if(!collection.guard()) return emit(false);
    HRESULT initialized=CoInitializeEx(nullptr,COINIT_MULTITHREADED);
    if(FAILED(initialized)) {collection.stage="com";return emit(false);}
    struct Uninitialize {
        Collection& collection;
        ~Uninitialize(){collection.held.clear();collection.walker.Reset();collection.automation.Reset();CoUninitialize();}
    } uninitialize{collection};
    ComPtr<IUIAutomation2> automation;
    if(FAILED(CoCreateInstance(CLSID_CUIAutomation8,nullptr,CLSCTX_INPROC_SERVER,IID_PPV_ARGS(&automation)))
        || FAILED(automation->put_ConnectionTimeout(100)) || FAILED(automation->put_TransactionTimeout(100))) {collection.stage="com";return emit(false);}
    if(FAILED(automation.As(&collection.automation)) || FAILED(automation->get_RawViewWalker(&collection.walker))) return emit(false);
    ComPtr<IUIAutomationElement> root;
    if(FAILED(automation->ElementFromHandle(r.window,&root)) || !root) return emit(false);
    if(!collection.append(root.Get(),0)) return emit(false);
    ComPtr<IUIAutomationElement> fresh_root;BOOL same=FALSE;
    if(FAILED(automation->ElementFromHandle(r.window,&fresh_root)) || !fresh_root
        || FAILED(automation->CompareElements(root.Get(),fresh_root.Get(),&same)) || !same) {
        collection.stage="root-replaced";return emit(false);
    }
    if(!collection.guard()) return emit(false);
    return emit(true);
}
#else
int windows_claude_uia_inventory() {return 5;}
#endif
