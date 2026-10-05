#include "uia_request_frame.hpp"
#include "uia_chat_scope.hpp"
#include "uia_current_mode.hpp"
#include "uia_chat_capability.hpp"
#include "uia_attachment.hpp"
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
#include <memory>
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
    bool collect_chat=false;
    std::size_t chat_text_units=0;
    std::vector<UiaChatScopeNode> chat_nodes;
    std::vector<ComPtr<IUIAutomationElement>> send_controls;
    ~Collection() { for(auto& node:chat_nodes)if(!node.label.empty())SecureZeroMemory(node.label.data(),node.label.size()*sizeof(wchar_t)); }
    const char* stage = "query";
    ProcessHandle root_process;
    std::uint64_t root_creation{};
    struct RetainedChild {
        UiaRetainedIdentity identity;
        std::unique_ptr<ProcessHandle> process;
    };
    std::vector<RetainedChild> retained_children;
    struct ModeSample {
        UiaModeCounts counts;
        std::vector<ComPtr<IUIAutomationElement>> identities;
        std::vector<ComPtr<IUIAutomationElement>> groups;
        bool available = true;
    };
    std::vector<ComPtr<IUIAutomationElement>> classic_controls, start_controls;
    const char* capability_status = "unavailable";
    UiaChatCapability capability;
    ModeSample initial_mode;
    const char* mode_status = "unavailable";
    UiaModeCounts mode_counts;
    bool project_mode(IUIAutomationElement* element, int type, const std::wstring& label,
                      bool& in_mode, ModeSample& sample, BOOL offscreen, BOOL enabled) {
        if (type == UIA_GroupControlTypeId && label == L"Mode") {
            BOOL group_offscreen = TRUE, group_enabled = FALSE;
            if (FAILED(element->get_CurrentIsOffscreen(&group_offscreen))
                || FAILED(element->get_CurrentIsEnabled(&group_enabled))) return false;
            if (group_offscreen || !group_enabled) return within();
            ++sample.counts.groups;
            ComPtr<IUIAutomationElement> retained = element;
            sample.identities.push_back(retained);
            sample.groups.push_back(retained);
            in_mode = true;
        }
        if (in_mode && type == UIA_ButtonControlTypeId && (label == L"Chat" || label == L"Cowork")) {
            if (!offscreen && enabled) {
                VARIANT properties; VariantInit(&properties);
                HRESULT result = element->GetCurrentPropertyValue(UIA_AriaPropertiesPropertyId, &properties);
                std::optional<bool> current;
                if (SUCCEEDED(result) && properties.vt == VT_BSTR) {
                    const unsigned length = properties.bstrVal ? SysStringLen(properties.bstrVal) : 0;
                    if (length <= 2048) {
                        std::wstring aria(properties.bstrVal ? properties.bstrVal : L"", length);
                        current = uia_current_page(aria);
                        if (!aria.empty()) SecureZeroMemory(aria.data(), aria.size() * sizeof(wchar_t));
                    }
                    if (properties.bstrVal) SecureZeroMemory(properties.bstrVal, length * sizeof(wchar_t));
                }
                VariantClear(&properties);
                if (!current) return false;
                if (label == L"Chat") { ++sample.counts.chat; sample.counts.current_chat += *current; }
                else { ++sample.counts.cowork; sample.counts.current_cowork += *current; }
                ComPtr<IUIAutomationElement> retained = element;
                sample.identities.push_back(retained);
            }
        }
        return within();
    }
    bool mode_walk(IUIAutomationElement* element, unsigned depth, bool in_mode,
                   ModeSample& sample, unsigned& visited) {
        if (!within() || depth > 32 || ++visited > 64) return false;
        int type = 0, pid = 0;
        BSTR name = nullptr;
        if (FAILED(element->get_CurrentControlType(&type))
            || FAILED(element->get_CurrentProcessId(&pid))
            || FAILED(element->get_CurrentName(&name))) {
            if (name) SysFreeString(name);
            return false;
        }
        std::wstring label;
        if (name) {
            const unsigned length = SysStringLen(name);
            if (length <= 65536) label.assign(name, length);
            SecureZeroMemory(name, length * sizeof(wchar_t)); SysFreeString(name);
            if (length > 65536) return false;
        }
        struct WipeLabel { std::wstring& value; ~WipeLabel() {
            if (!value.empty()) SecureZeroMemory(value.data(), value.size() * sizeof(wchar_t));
        }} wipe{label};
        bool owned = pid == static_cast<int>(request.pid);
        for (const auto& child : retained_children) owned = owned || pid == static_cast<int>(child.identity.pid);
        if (!owned) return false;
        BOOL offscreen = TRUE, enabled = FALSE;
        if (type == UIA_ButtonControlTypeId &&
            (FAILED(element->get_CurrentIsOffscreen(&offscreen))
             || FAILED(element->get_CurrentIsEnabled(&enabled)))) return false;
        if (!project_mode(element, type, label, in_mode, sample, offscreen, enabled)) return false;
        ComPtr<IUIAutomationElement> child;
        if (FAILED(walker->GetFirstChildElement(element, &child))) return false;
        while (child) {
            if (!mode_walk(child.Get(), depth + 1, in_mode, sample, visited)) return false;
            ComPtr<IUIAutomationElement> next;
            if (FAILED(walker->GetNextSiblingElement(child.Get(), &next))) return false;
            child = next;
        }
        return within();
    }
    bool mode_attached(IUIAutomationElement* element, IUIAutomationElement* root) {
        ComPtr<IUIAutomationElement> retained = element, retained_root = root;
        const auto parent = [&](const ComPtr<IUIAutomationElement>& child)
            -> std::optional<ComPtr<IUIAutomationElement>> {
            if (!within()) return std::nullopt;
            ComPtr<IUIAutomationElement> value;
            if (FAILED(walker->GetParentElement(child.Get(), &value)) || !value || !within())
                return std::nullopt;
            return value;
        };
        const auto equal = [&](const ComPtr<IUIAutomationElement>& left,
                               const ComPtr<IUIAutomationElement>& right) -> std::optional<bool> {
            if (!within()) return std::nullopt;
            BOOL same = FALSE;
            if (FAILED(automation->CompareElements(left.Get(), right.Get(), &same)) || !within())
                return std::nullopt;
            return same != FALSE;
        };
        const auto owned = [&](const ComPtr<IUIAutomationElement>& node) {
            if (!within()) return false;
            int pid = 0;
            if (FAILED(node->get_CurrentProcessId(&pid)) || !within()) return false;
            if (pid == static_cast<int>(request.pid)) return true;
            for (const auto& child : retained_children)
                if (pid == static_cast<int>(child.identity.pid)) return true;
            return false;
        };
        return uia_attached_to_root(retained, retained_root, parent, equal, owned,
                                    [&] { return within(); });
    }
    void observe_mode(IUIAutomationElement* root) {
        // Global uniqueness was measured by the existing complete inventory.
        // Re-read only the retained source Mode subtrees, never the whole app.
        const auto& first = initial_mode;
        if (!first.available || first.groups.size() > 32 || !guard()) return;
        ModeSample second;
        unsigned visited = 0;
        for (const auto& group : first.groups) {
            if (!mode_attached(group.Get(), root)
                || !mode_walk(group.Get(), 0, false, second, visited)
                || !mode_attached(group.Get(), root)) return;
        }
        if (!guard()) return;
        ComPtr<IUIAutomationElement> current_root;
        BOOL root_same = FALSE;
        if (FAILED(automation->ElementFromHandle(request.window, &current_root)) || !current_root
            || FAILED(automation->CompareElements(root, current_root.Get(), &root_same))) return;
        bool unchanged = root_same && first.counts == second.counts
            && first.identities.size() == second.identities.size();
        for (std::size_t index = 0; unchanged && index < first.identities.size(); ++index) {
            BOOL same = FALSE;
            if (FAILED(automation->CompareElements(first.identities[index].Get(), second.identities[index].Get(), &same))) return;
            unchanged = same;
        }
        mode_status = uia_mode_status(first.counts, true, unchanged);
        if (unchanged) mode_counts = first.counts;
    }
    bool source_control(IUIAutomationElement* element, int expected_type, const wchar_t* expected_name,
                        std::array<long,4>& bounds, bool& enabled) {
        if (!within()) return false;
        int type=0; BOOL offscreen=TRUE, active=FALSE; RECT rect{}; BSTR name=nullptr;
        if (FAILED(element->get_CurrentControlType(&type)) || type!=expected_type
            || FAILED(element->get_CurrentName(&name))) { if(name) SysFreeString(name); return false; }
        const bool matches=name && SysStringLen(name)<=128 && std::wstring(name,SysStringLen(name))==expected_name;
        if(name) { SecureZeroMemory(name,SysStringLen(name)*sizeof(wchar_t)); SysFreeString(name); }
        if (!matches || FAILED(element->get_CurrentIsOffscreen(&offscreen)) || offscreen
            || FAILED(element->get_CurrentIsEnabled(&active))
            || FAILED(element->get_CurrentBoundingRectangle(&rect)) || !contains(request.bounds,rect)
            || !within()) return false;
        bounds={rect.left,rect.top,rect.right,rect.bottom}; enabled=active;
        return true;
    }
    bool read_capability(IUIAutomationElement* editor, IUIAutomationElement* start,
                         UiaChatCapability& result) {
        if (!within()) return false;
        if (!source_control(editor,UIA_EditControlTypeId,L"Write your prompt to Claude",result.editor_bounds,result.editor_enabled)
            || !result.editor_enabled
            || !source_control(start,UIA_ButtonControlTypeId,L"Start task",result.start_bounds,result.start_enabled)) return false;
        BOOL password=TRUE, keyboard=FALSE;
        if (FAILED(editor->get_CurrentIsPassword(&password))
            || FAILED(editor->get_CurrentIsKeyboardFocusable(&keyboard)) || !within()) return false;
        result.password=password; result.keyboard_focusable=keyboard;
        ComPtr<IUIAutomationValuePattern> value;
        HRESULT queried=editor->GetCurrentPatternAs(UIA_ValuePatternId, IID_PPV_ARGS(&value));
        if (FAILED(queried) && queried != UIA_E_NOTSUPPORTED) return false;
        result.value_pattern=value.Get() != nullptr;
        if (value) {
            BOOL read_only=TRUE; BSTR text=nullptr;
            if (FAILED(value->get_CurrentIsReadOnly(&read_only))) return false;
            HRESULT read=value->get_CurrentValue(&text);
            const unsigned length=text ? SysStringLen(text) : 0;
            if (text) { SecureZeroMemory(text,length*sizeof(wchar_t)); SysFreeString(text); }
            if (FAILED(read) || length>65536 || !within()) return false;
            result.read_only=read_only; result.value_empty=length==0;
        }
        ComPtr<IUIAutomationInvokePattern> invoke;
        queried=start->GetCurrentPatternAs(UIA_InvokePatternId, IID_PPV_ARGS(&invoke));
        if (FAILED(queried) && queried != UIA_E_NOTSUPPORTED) return false;
        result.invoke_pattern=invoke.Get() != nullptr;
        return uia_chat_capability_valid(result) && within();
    }
    void observe_capability(IUIAutomationElement* root) {
        if (std::string(mode_status)=="unavailable" || std::string(mode_status)=="changed") {
            capability_status=mode_status; return;
        }
        const bool chat=std::string(mode_status)=="chat";
        capability_status=uia_chat_capability_status(static_cast<unsigned>(classic_controls.size()),static_cast<unsigned>(start_controls.size()),chat,true,true);
        if (std::string(capability_status)!="observed") return;
        capability_status="unavailable";
        const auto editor=classic_controls.front(), start=start_controls.front();
        const auto attached=[&] { return mode_attached(editor.Get(),root) && mode_attached(start.Get(),root); };
        UiaChatCapability first,second;
        if (!guard() || !attached() || !read_capability(editor.Get(),start.Get(),first)
            || !attached() || !guard() || !read_capability(editor.Get(),start.Get(),second)
            || !attached() || !guard()) return;
        ComPtr<IUIAutomationElement> fresh; BOOL same=FALSE;
        if (FAILED(automation->ElementFromHandle(request.window,&fresh)) || !fresh
            || FAILED(automation->CompareElements(root,fresh.Get(),&same)) || !within()) return;
        capability_status=uia_chat_capability_status(1,1,chat,true,same && first==second);
        if (std::string(capability_status)=="observed") capability=first;
    }
    bool retained_identity() const {
        std::vector<UiaRetainedIdentity> identities;
        for (const auto& child : retained_children) identities.push_back(child.identity);
        const auto query = [&](std::uint32_t pid, std::uint64_t& time) {
            if (!within()) return false;
            if (pid == request.pid) return live_creation(root_process.value, time) && within();
            for (const auto& child : retained_children)
                if (child.identity.pid == pid) return live_creation(child.process->value, time) && within();
            return false;
        };
        return uia_collection_identity(request.pid, root_creation, identities, query) && within();
    }
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
            if (++count>1024) {stage="limit-windows";return false;}
            if (!IsWindowVisible(above) || IsIconic(above)) continue;
            DWORD c=0,pid=0;RECT front{},intersection{};
            if (FAILED(DwmGetWindowAttribute(above,DWMWA_CLOAKED,&c,sizeof(c)))) {stage="query";return false;}
            if(c) continue;
            if(!GetWindowThreadProcessId(above,&pid) || !GetWindowRect(above,&front)) {stage="query";return false;}
            if(pid==request.pid || IntersectRect(&intersection,&front,&bounds)) {stage="occlusion";return false;}
        }
        if (!retained_identity()) {stage="descendant-correlation-unavailable";return false;}
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
    const char* retain_descendant(DWORD child) {
        constexpr auto unavailable = "descendant-correlation-unavailable";
        std::uint64_t before = 0;
        if (!within() || !live_creation(root_process.value, before) || before != root_creation)
            return unavailable;
        for (const auto& prior : retained_children) {
            if (prior.identity.pid == child) {
                std::uint64_t current = 0;
                return live_creation(prior.process->value, current)
                    && current == prior.identity.creation && guard() ? nullptr : unavailable;
            }
        }
        if (retained_children.size() >= 64) {stage="limit-processes";return "limit-processes";}
        auto process = std::make_unique<ProcessHandle>(
            OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, FALSE, child));
        std::uint64_t child_creation = 0;
        if (!live_creation(process->value, child_creation) || !within()) return unavailable;
        std::vector<CorrelationEntry> rows, confirm;
        if (!process_snapshot(rows) || !process_snapshot(confirm)) return unavailable;
        const auto query = [&](std::uint32_t pid, std::uint64_t& time) {
            if (!within()) return false;
            if (pid == child) return live_creation(process->value, time) && within();
            if (pid == request.pid) return live_creation(root_process.value, time) && within();
            ProcessHandle parent(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, FALSE, pid));
            return live_creation(parent.value, time) && within();
        };
        const auto relation = classify_uia_ancestry(rows, confirm, child, request.pid,
            root_creation, child_creation, query);
        std::uint64_t after = 0;
        if (!within() || !live_creation(root_process.value, after) || after != root_creation
            || !guard()) return unavailable;
        switch (relation) {
            case UiaAncestry::Owned:
                retained_children.push_back({{child, child_creation}, std::move(process)});
                return retained_identity() ? nullptr : unavailable;
            case UiaAncestry::Foreign: return "foreign-descendant-process";
            case UiaAncestry::Unavailable: return unavailable;
        }
        return unavailable;
    }
    bool append(IUIAutomationElement* element,unsigned depth, bool in_mode = false,int parent=-1) {
        if(!within()) {stage="deadline";return false;}
        if(depth>32) {stage="limit-depth";return false;}
        if(++nodes>1024) {stage="limit-nodes";return false;}
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
            const char* failure = depth>0 && pid>0 && static_cast<DWORD>(pid)!=request.pid
                ? retain_descendant(static_cast<DWORD>(pid)) : rejected;
            if (failure) {stage=failure;return false;}
        }
        if(FAILED(element->get_CurrentControlType(&type)) || FAILED(element->get_CurrentName(&name))) return false;
        std::wstring text;
        if(name) {
            unsigned length=SysStringLen(name);
            if(length>65536) {SecureZeroMemory(name,length*sizeof(wchar_t));SysFreeString(name);stage="limit-name";return false;}
            text.assign(name,length);SecureZeroMemory(name,length*sizeof(wchar_t));SysFreeString(name);
        }
        struct Wipe {std::wstring& text;~Wipe(){if(!text.empty())SecureZeroMemory(text.data(),text.size()*sizeof(wchar_t));}} wipe{text};
        BOOL offscreen=TRUE,enabled=FALSE;
        if(type==UIA_EditControlTypeId || type==UIA_ButtonControlTypeId) {
            if(FAILED(element->get_CurrentIsOffscreen(&offscreen)) || FAILED(element->get_CurrentIsEnabled(&enabled))) return false;
        }
        if (initial_mode.available && !project_mode(element, type, text, in_mode, initial_mode, offscreen, enabled))
            initial_mode.available = false;
        if(type==UIA_EditControlTypeId && !offscreen && enabled) {
            classic+=text==L"Write your prompt to Claude";modern+=text==L"Message";
            if(text==L"Write your prompt to Claude") classic_controls.emplace_back(element);
        }
        if(type==UIA_ButtonControlTypeId && !offscreen) {
            sends+=text==L"Send message";starts+=text==L"Start task";copies+=text==L"Copy";
            if(text==L"Start task") start_controls.emplace_back(element);
            if(collect_chat && text==L"Send message")send_controls.emplace_back(element);
        }
        bool is_heading=false;
        if(type==UIA_TextControlTypeId) {
            VARIANT heading;VariantInit(&heading);
            HRESULT result=element->GetCurrentPropertyValue(UIA_HeadingLevelPropertyId,&heading);
            bool valid=SUCCEEDED(result) && heading.vt==VT_I4;
            is_heading=valid && heading.lVal>=HeadingLevel1 && heading.lVal<=HeadingLevel9;
            if(is_heading && text.rfind(L"Claude responded:",0)==0) ++headings;
            VariantClear(&heading);
            if(!valid) {stage="heading-property";return false;}
        }
        const int index=static_cast<int>(held.size());
        if(collect_chat) {
            const auto chat_role=is_heading?UiaChatRole::Heading
                :type==UIA_ButtonControlTypeId?UiaChatRole::Button
                :type==UIA_TextControlTypeId?UiaChatRole::Text
                :type==UIA_GroupControlTypeId?UiaChatRole::Group
                :(type==UIA_DocumentControlTypeId || type==UIA_WindowControlTypeId || type==UIA_PaneControlTypeId)?UiaChatRole::Boundary
                :UiaChatRole::Other;
            const bool retain_label=uia_chat_retains_label(chat_role,text);
            chat_text_units+=retain_label?text.size():0;
            if(chat_text_units>65536){stage="limit-text";return false;}
            chat_nodes.push_back({chat_role,retain_label?text:L"",parent});
        }
        ComPtr<IUIAutomationElement> retained=element;held.push_back(retained);
        ComPtr<IUIAutomationElement> child;
        if(FAILED(walker->GetFirstChildElement(element,&child))) return false;
        while(child) {
            if(!append(child.Get(),depth+1,in_mode,index)) return false;
            ComPtr<IUIAutomationElement> next;
            if(FAILED(walker->GetNextSiblingElement(child.Get(),&next))) return false;
            child=next;
        }
        return within();
    }
};
}
#include "windows_claude_chat_turn.inc"

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
        if (complete) {
            std::cout << " mode " << collection.mode_status;
            if (std::string(collection.mode_status) == "unavailable" || std::string(collection.mode_status) == "changed")
                std::cout << " - - - - -";
            else std::cout << ' ' << collection.mode_counts.groups << ' ' << collection.mode_counts.chat
                << ' ' << collection.mode_counts.cowork << ' ' << collection.mode_counts.current_chat
                << ' ' << collection.mode_counts.current_cowork;
            std::cout << " capability " << collection.capability_status;
            if (std::string(collection.capability_status)!="observed") std::cout << " - - - - - -";
            else {
                const auto& c=collection.capability;
                const auto optional=[](std::optional<bool> value) { return value ? (*value ? "1" : "0") : "-"; };
                std::cout << ' ' << c.value_pattern << ' ' << optional(c.read_only)
                    << ' ' << optional(c.value_empty) << ' ' << c.password
                    << ' ' << c.keyboard_focusable << ' ' << c.invoke_pattern;
            }
        }
        std::cout<<'\n';return std::cout?0:4;
    };
    if(!collection.guard()) return emit(false);
    HRESULT initialized=CoInitializeEx(nullptr,COINIT_MULTITHREADED);
    if(FAILED(initialized)) {collection.stage="com";return emit(false);}
    struct Uninitialize {
        Collection& collection;
        ~Uninitialize(){collection.classic_controls.clear();collection.start_controls.clear();collection.held.clear();collection.walker.Reset();collection.automation.Reset();CoUninitialize();}
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
    collection.observe_mode(fresh_root.Get());
    collection.observe_capability(fresh_root.Get());
    if(!collection.guard()) return emit(false);
    return emit(true);
}
#else
int windows_claude_uia_inventory() {return 5;}
int windows_claude_chat_turn() {return 5;}
#endif
