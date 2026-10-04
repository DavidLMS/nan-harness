// Separate action command; the passive inventory entry never reaches this code.
#include "uia_chat_input.hpp"
#include <thread>
namespace {
struct ChatInput {
    Collection collection;
    ComPtr<IUIAutomationElement> root,editor,send;
    ComPtr<IUIAutomationValuePattern> value;
    ComPtr<IUIAutomationInvokePattern> invoke;
    ProcessHandle checker;
    DWORD checker_pid{};
    std::uint64_t cutoff{}, checker_creation{};
    std::wstring prompt,sentinel,marker,prior;
    std::string mode;
    const wchar_t* send_name=L"Start task";
    UiaChatCapability original;
    const char* stage="request";
    ~ChatInput() {
        if(!prompt.empty())SecureZeroMemory(prompt.data(),prompt.size()*sizeof(wchar_t));
        if(!sentinel.empty())SecureZeroMemory(sentinel.data(),sentinel.size()*sizeof(wchar_t));
        if(!marker.empty())SecureZeroMemory(marker.data(),marker.size()*sizeof(wchar_t));
        if(!prior.empty())SecureZeroMemory(prior.data(),prior.size()*sizeof(wchar_t));
    }
    bool time_left() const { return collection.within() && GetTickCount64()<cutoff; }
    bool within() const {
        std::uint64_t created=0;
        return time_left() && live_creation(checker.value,created) && created==checker_creation;
    }
    bool owned() {
        if(!within() || !collection.guard())return false;
        ComPtr<IUIAutomationElement> current;BOOL same=FALSE;
        return SUCCEEDED(collection.automation->ElementFromHandle(collection.request.window,&current))
            && current && SUCCEEDED(collection.automation->CompareElements(root.Get(),current.Get(),&same))
            && same && within();
    }
    bool unique_source(IUIAutomationElement* retained,int type,const wchar_t* name) {
        if(!within())return false;
        VARIANT label;VariantInit(&label);label.vt=VT_BSTR;label.bstrVal=SysAllocString(name);
        VARIANT role;VariantInit(&role);role.vt=VT_I4;role.lVal=type;
        ComPtr<IUIAutomationCondition> by_name,by_role,condition;
        const bool made=label.bstrVal && SUCCEEDED(collection.automation->CreatePropertyCondition(UIA_NamePropertyId,label,&by_name))
            && SUCCEEDED(collection.automation->CreatePropertyCondition(UIA_ControlTypePropertyId,role,&by_role))
            && SUCCEEDED(collection.automation->CreateAndCondition(by_name.Get(),by_role.Get(),&condition));
        VariantClear(&label);
        if(!made || !within())return false;
        ComPtr<IUIAutomationElementArray> matches;
        if(FAILED(root->FindAll(TreeScope_Descendants,condition.Get(),&matches)) || !matches)return false;
        int count=0;if(FAILED(matches->get_Length(&count)) || count!=1 || !within())return false;
        ComPtr<IUIAutomationElement> fresh;BOOL same=FALSE;
        return SUCCEEDED(matches->GetElement(0,&fresh)) && fresh
            && SUCCEEDED(collection.automation->CompareElements(retained,fresh.Get(),&same))
            && same && collection.mode_attached(fresh.Get(),root.Get()) && within();
    }
    bool target(bool submitting=false) {
        if(!owned() || !collection.mode_attached(editor.Get(),root.Get())
            || !collection.mode_attached(send.Get(),root.Get())
            || !unique_source(editor.Get(),UIA_EditControlTypeId,L"Write your prompt to Claude")
            || !unique_source(send.Get(),UIA_ButtonControlTypeId,send_name))return false;
        std::array<long,4> editor_bounds{},send_bounds{};bool editor_enabled=false,send_enabled=false;
        if(!collection.source_control(editor.Get(),UIA_EditControlTypeId,L"Write your prompt to Claude",editor_bounds,editor_enabled)
            || !editor_enabled || editor_bounds!=original.editor_bounds
            || !collection.source_control(send.Get(),UIA_ButtonControlTypeId,send_name,send_bounds,send_enabled)
            || send_bounds!=original.start_bounds || (submitting&&!send_enabled))return false;
        BOOL read_only=TRUE,password=TRUE;
        if(FAILED(value->get_CurrentIsReadOnly(&read_only)) || read_only
            || FAILED(editor->get_CurrentIsPassword(&password)) || password)return false;
        collection.observe_mode(root.Get());
        return std::string(collection.mode_status)=="chat" && owned();
    }
    UiaChatInputValue focus() {
        if(!target())return UiaChatInputValue::Rejected;
        ComPtr<IUIAutomationElement> focused;BOOL same=FALSE;
        if(FAILED(collection.automation->GetFocusedElement(&focused)) || !focused
            || FAILED(collection.automation->CompareElements(editor.Get(),focused.Get(),&same)))return UiaChatInputValue::Rejected;
        const bool attached=collection.mode_attached(focused.Get(),root.Get());
        return uia_chat_focus_state(true,true,same!=FALSE,attached,owned());
    }
    bool wait_focus() {
        return uia_chat_settle([&]{return focus();},[&]{return within();},
            []{std::this_thread::sleep_for(std::chrono::milliseconds(20));});
    }
    bool key(WORD code) {
        // No modifier cleanup or repeated key is permitted on uncertain delivery.
        if(!wait_focus() || !target() || GetAsyncKeyState(VK_CONTROL)&0x8000
            || GetAsyncKeyState(VK_SHIFT)&0x8000 || GetAsyncKeyState(VK_MENU)&0x8000
            || GetAsyncKeyState(VK_LWIN)&0x8000 || GetAsyncKeyState(VK_RWIN)&0x8000)return false;
        INPUT keys[4]{};
        for(auto& item:keys)item.type=INPUT_KEYBOARD;
        keys[0].ki.wVk=VK_CONTROL;keys[1].ki.wVk=code;
        keys[2].ki.wVk=code;keys[2].ki.dwFlags=KEYEVENTF_KEYUP;
        keys[3].ki.wVk=VK_CONTROL;keys[3].ki.dwFlags=KEYEVENTF_KEYUP;
        return within() && SendInput(4,keys,sizeof(INPUT))==4;
    }
    const char* clipboard_write(const std::wstring& text) {
        if(!owned())return time_left()?"clipboard-guard-before":"clipboard-deadline-before";
        HWND owner=CreateWindowExW(0,L"STATIC",L"",0,0,0,0,0,HWND_MESSAGE,nullptr,GetModuleHandleW(nullptr),nullptr);
        if(!owner)return "clipboard-owner";
        const auto bytes=(text.size()+1)*sizeof(wchar_t);
        HGLOBAL memory=GlobalAlloc(GMEM_MOVEABLE,bytes);bool transferred=false;
        const char* failure=memory?nullptr:"clipboard-allocation";
        if(memory) {
            void* locked=GlobalLock(memory);
            if(!locked)failure="clipboard-lock";
            else {
                memcpy(locked,text.c_str(),bytes);GlobalUnlock(memory);
                const bool opened=uia_chat_acquire_clipboard([&]{return OpenClipboard(owner)!=FALSE;},
                    [&]{return within();},[&]{return collection.guard();},
                    []{std::this_thread::sleep_for(std::chrono::milliseconds(20));});
                if(!opened)failure=time_left()?"clipboard-guard-before":"clipboard-open-deadline";
                else {
                    // An acquisition may complete after the original cutoff.
                    // Close its lock, but never Empty/Set in that case.
                    if(!within())failure="clipboard-open-deadline";
                    else if(!collection.guard())failure=time_left()?"clipboard-guard-before":"clipboard-open-deadline";
                    else if(!EmptyClipboard())failure="clipboard-empty";
                    else if(!within())failure="clipboard-open-deadline";
                    else if(!SetClipboardData(CF_UNICODETEXT,memory))failure="clipboard-set";
                    else transferred=true;
                    const bool closed=CloseClipboard()!=FALSE;
                    if(!closed && !failure)failure="clipboard-close";
                }
            }
            if(!transferred) {
                if(void* buffer=GlobalLock(memory)){SecureZeroMemory(buffer,bytes);GlobalUnlock(memory);}
                GlobalFree(memory);
            }
        }
        DestroyWindow(owner);
        if(failure)return failure;
        return owned()?nullptr:time_left()?"clipboard-guard-after":"clipboard-deadline-after";
    }
    std::optional<bool> clipboard_matches(const std::wstring& expected) {
        if(!owned() || !OpenClipboard(nullptr))return std::nullopt;
        bool matched=false,valid=false;
        HANDLE memory=GetClipboardData(CF_UNICODETEXT);
        const auto bytes=memory?GlobalSize(memory):0;
        if(bytes && bytes<=131072) {
            auto text=static_cast<const wchar_t*>(GlobalLock(memory));
            if(text) {
                const auto capacity=bytes/sizeof(wchar_t);std::size_t length=0;
                while(length<capacity && text[length])++length;
                valid=length<capacity;
                matched=valid && length==expected.size() && std::equal(expected.begin(),expected.end(),text);
                GlobalUnlock(memory);
            }
        }
        const bool closed=CloseClipboard();
        if(!closed || !valid || !owned())return std::nullopt;
        return matched;
    }
    UiaChatInputValue pasted() {
        if(focus()!=UiaChatInputValue::Ready)return UiaChatInputValue::Rejected;
        BSTR text=nullptr;const HRESULT result=value->get_CurrentValue(&text);
        const unsigned length=text?SysStringLen(text):0;
        std::wstring observed;
        if(SUCCEEDED(result) && length<=1024 && text)observed.assign(text,length);
        if(text){SecureZeroMemory(text,length*sizeof(wchar_t));SysFreeString(text);}
        const bool exact=focus()==UiaChatInputValue::Ready;
        const auto state=uia_chat_input_value(observed,prompt,prior,SUCCEEDED(result)&&length<=1024,exact);
        if(!observed.empty())SecureZeroMemory(observed.data(),observed.size()*sizeof(wchar_t));
        return state;
    }
    bool action_control(IUIAutomationElement* button,const wchar_t* name,
                        const std::array<long,4>& original_bounds) {
        if(!owned() || !collection.mode_attached(button,root.Get()))return false;
        std::array<long,4> bounds{};bool enabled=false;
        return collection.source_control(button,UIA_ButtonControlTypeId,name,bounds,enabled)
            && enabled && bounds==original_bounds && owned();
    }
    bool fresh_scope(const UiaChatScope& original_scope,IUIAutomationElement* control,
                     const wchar_t* name,const std::array<long,4>& bounds) {
        if(!action_control(control,name,bounds))return false;
        Collection fresh;fresh.request=collection.request;fresh.deadline=collection.deadline;
        fresh.collect_chat=true;fresh.automation=collection.automation;fresh.walker=collection.walker;
        fresh.root_process.value=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION|SYNCHRONIZE,FALSE,fresh.request.pid);
        if(!live_creation(fresh.root_process.value,fresh.root_creation)
            || fresh.root_creation!=collection.root_creation)return false;
        ComPtr<IUIAutomationElement> current;BOOL same=FALSE;
        if(FAILED(fresh.automation->ElementFromHandle(fresh.request.window,&current)) || !current
            || FAILED(fresh.automation->CompareElements(root.Get(),current.Get(),&same)) || !same
            || !fresh.guard() || !fresh.append(current.Get(),0))return false;
        fresh.observe_mode(current.Get());
        const auto scope=mode=="failure-details"?uia_chat_failure_details(fresh.chat_nodes,prompt)
            :uia_chat_scope(fresh.chat_nodes,prompt,marker,mode!="copy");
        if(std::string(fresh.mode_status)!="chat" || scope.control<0)return false;
        for(const auto pair:{std::pair<int,int>{original_scope.control,scope.control},
                            {original_scope.anchor,scope.anchor},{original_scope.ancestor,scope.ancestor}}) {
            BOOL equal=FALSE;
            if(pair.first<0 || pair.second<0
                || FAILED(fresh.automation->CompareElements(collection.held[pair.first].Get(),fresh.held[pair.second].Get(),&equal))
                || !equal)return false;
        }
        if(mode=="failure-details") {
            BOOL equal=FALSE;
            if(original_scope.retry<0 || scope.retry<0 || original_scope.user_heading<0 || scope.user_heading<0
                || FAILED(fresh.automation->CompareElements(collection.held[original_scope.user_heading].Get(),
                    fresh.held[scope.user_heading].Get(),&equal)) || !equal)return false;
            equal=FALSE;
            std::array<long,4> retry_bounds{};bool retry_enabled=false;
            if(FAILED(fresh.automation->CompareElements(collection.held[original_scope.retry].Get(),
                fresh.held[scope.retry].Get(),&equal)) || !equal
                || !fresh.source_control(fresh.held[scope.retry].Get(),UIA_ButtonControlTypeId,L"Retry",retry_bounds,retry_enabled)
                || !retry_enabled)return false;
        }
        return fresh.guard() && action_control(control,name,bounds) && within();
    }
    const char* action() {
        if(marker.empty() || (mode!="copy" && marker!=L"NAN_CHECK_EXPECTED_FAILURE"))return "scope";
        const auto scope=mode=="failure-details"?uia_chat_failure_details(collection.chat_nodes,prompt)
            :uia_chat_scope(collection.chat_nodes,prompt,marker,mode!="copy");
        if(scope.control<0)return scope.failure;
        const wchar_t* name=mode=="copy"?L"Copy":mode=="failure-details"?L"View details":L"Retry";
        const auto control=collection.held[scope.control];
        std::array<long,4> bounds{};bool enabled=false;
        if(!collection.source_control(control.Get(),UIA_ButtonControlTypeId,name,bounds,enabled)
            || !enabled || !fresh_scope(scope,control.Get(),name,bounds))return "control";
        if(mode=="retry-ready")return "retry-ready";
        if(mode=="copy")if(const char* failure=clipboard_write(sentinel))return failure;
        ComPtr<IUIAutomationInvokePattern> action;
        if(FAILED(control->GetCurrentPatternAs(UIA_InvokePatternId,IID_PPV_ARGS(&action))) || !action
            || !fresh_scope(scope,control.Get(),name,bounds) || !within())return "control";
        // The parent seals Copy/Retry before this request. This process invokes
        // exactly once and never retries an uncertain HRESULT or postcondition.
        const HRESULT delivered=action->Invoke();
        if(FAILED(delivered) || !within() || !owned())return "action-uncertain";
        if(mode=="retry")return "retried";
        if(mode=="failure-details")return "failure-details-opened";
        while(within()) {
            if(!owned())return "action-uncertain";
            const auto copied=clipboard_matches(marker);
            if(!copied)return "response-mismatch";
            if(*copied)return "copied";
            std::this_thread::sleep_for(std::chrono::milliseconds(20));
        }
        return "response-mismatch";
    }
    const char* input() {
        if(!target())return "control";
        if(FAILED(editor->SetFocus()))return "input-focus-setting";
        if(!wait_focus())return "input-focused-identity";
        if(!key('A'))return "input-replace-select-key";
        if(const char* failure=clipboard_write(prompt))return failure;
        if(focus()!=UiaChatInputValue::Ready)return "input-focused-identity";
        BSTR previous=nullptr;
        const HRESULT previous_result=value->get_CurrentValue(&previous);
        const unsigned previous_size=previous?SysStringLen(previous):0;
        if(SUCCEEDED(previous_result) && previous_size<=1024 && previous)prior.assign(previous,previous_size);
        if(previous){SecureZeroMemory(previous,previous_size*sizeof(wchar_t));SysFreeString(previous);}
        if(FAILED(previous_result) || previous_size>1024 || focus()!=UiaChatInputValue::Ready)return "input-value-mismatch";
        if(!key('V'))return "input-paste-key";
        if(!uia_chat_settle([&]{return pasted();},[&]{return within();},
            []{std::this_thread::sleep_for(std::chrono::milliseconds(20));}))return "input-value-mismatch";
        if(const char* failure=clipboard_write(sentinel))return failure;
        if(!key('A'))return "input-readback-select-key";
        if(!key('C'))return "input-readback-copy-key";
        // Clipboard copy may settle, but neither Copy nor any key is repeated.
        while(within()) {
            if(focus()!=UiaChatInputValue::Ready)return "input-focused-identity";
            const auto copied=clipboard_matches(prompt);
            if(!copied)return "input-clipboard-mismatch";
            if(*copied)break;
            std::this_thread::sleep_for(std::chrono::milliseconds(20));
        }
        if(!within() || clipboard_matches(prompt)!=true || pasted()!=UiaChatInputValue::Ready)return "input-clipboard-mismatch";
        if(!wait_focus() || !target(true))return "control";
        BOOL enabled=FALSE;
        if(FAILED(send->get_CurrentIsEnabled(&enabled)) || !enabled || !owned() || !within())return "control";
        // This is the only submission action. An HRESULT cannot prove receipt.
        const HRESULT submitted=invoke->Invoke();
        if(FAILED(submitted) || !within() || !owned())return "action-uncertain";
        return "sent";
    }
};
}
namespace {
bool chat_utf8(const std::string& bytes,std::wstring& result) {
    const int units=MultiByteToWideChar(CP_UTF8,MB_ERR_INVALID_CHARS,bytes.data(),static_cast<int>(bytes.size()),nullptr,0);
    if(units<=0 || units>1024)return false;
    result.resize(units);
    return MultiByteToWideChar(CP_UTF8,MB_ERR_INVALID_CHARS,bytes.data(),static_cast<int>(bytes.size()),result.data(),units)==units;
}
bool chat_parent(DWORD expected) {
    ProcessHandle snapshot(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS,0));
    if(snapshot.value==INVALID_HANDLE_VALUE)return false;
    PROCESSENTRY32W row{};row.dwSize=sizeof(row);
    if(!Process32FirstW(snapshot.value,&row))return false;
    unsigned count=0;bool matched=false;
    do {
        if(++count>4096)return false;
        if(row.th32ProcessID==GetCurrentProcessId())matched=row.th32ParentProcessID==expected;
    }while(Process32NextW(snapshot.value,&row));
    return GetLastError()==ERROR_NO_MORE_FILES && matched;
}
}
int windows_claude_chat_turn() {
    const auto started=GetTickCount64();
    std::cout<<"ready "<<started<<'\n'<<std::flush;
    if(!std::cout)return 4;
    auto wire=uia_chat_frame(std::cin);
    if(!wire)return 2;
    ChatInput turn;auto& c=turn.collection;c.collect_chat=true;
    std::uint64_t id=0;unsigned budget=0;std::string prompt_hex,sentinel_hex,marker_hex,extra;
    std::istringstream parser(*wire);
    const bool parsed=static_cast<bool>(parser>>id>>c.request.pid>>c.request.bounds.left
        >>c.request.bounds.top>>c.request.bounds.right>>c.request.bounds.bottom
        >>budget>>turn.cutoff>>turn.checker_pid>>turn.mode>>prompt_hex>>marker_hex>>sentinel_hex);
    if(!parsed || parser>>extra || !id || id>UINTPTR_MAX || !c.request.pid || !turn.checker_pid
        || (turn.mode!="input-replace-owned" && turn.mode!="copy" && turn.mode!="retry-ready" && turn.mode!="retry" && turn.mode!="failure-details")
        || budget<=50 || budget>15000 || turn.cutoff<=started || turn.cutoff-started>15000)return 2;
    auto prompt=uia_chat_hex(prompt_hex),sentinel=uia_chat_hex(sentinel_hex);
    auto marker=marker_hex=="-"?std::optional<std::string>(""):uia_chat_hex(marker_hex);
    SecureZeroMemory(wire->data(),wire->size());SecureZeroMemory(prompt_hex.data(),prompt_hex.size());
    SecureZeroMemory(sentinel_hex.data(),sentinel_hex.size());SecureZeroMemory(marker_hex.data(),marker_hex.size());
    if(!prompt || !sentinel || !marker)return 2;
    const bool valid=chat_utf8(*prompt,turn.prompt) && chat_utf8(*sentinel,turn.sentinel)
        && (marker->empty() || chat_utf8(*marker,turn.marker));
    SecureZeroMemory(prompt->data(),prompt->size());SecureZeroMemory(sentinel->data(),sentinel->size());
    if(!marker->empty())SecureZeroMemory(marker->data(),marker->size());
    if(!valid || turn.prompt==turn.sentinel)return 2;
    c.request.window=reinterpret_cast<HWND>(static_cast<std::uintptr_t>(id));
    c.deadline=Clock::now()+std::chrono::milliseconds(budget);
    c.root_process.value=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION|SYNCHRONIZE,FALSE,c.request.pid);
    turn.checker.value=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION|SYNCHRONIZE,FALSE,turn.checker_pid);
    live_creation(c.root_process.value,c.root_creation);live_creation(turn.checker.value,turn.checker_creation);
    const auto emit=[&](const char* stage) {
        if(std::string(stage)!="action-uncertain" && !turn.time_left())stage="deadline";
        std::cout<<"turn "<<stage<<'\n';return std::cout?0:4;
    };
    if(!chat_parent(turn.checker_pid) || !turn.within() || !c.guard())return emit("window");
    if(FAILED(CoInitializeEx(nullptr,COINIT_MULTITHREADED)))return emit("tree");
    struct ComCleanup {
        ChatInput& turn;
        ~ComCleanup() {
            turn.invoke.Reset();turn.value.Reset();turn.send.Reset();turn.editor.Reset();turn.root.Reset();
            auto& c=turn.collection;c.initial_mode.identities.clear();c.initial_mode.groups.clear();
            c.classic_controls.clear();c.start_controls.clear();c.send_controls.clear();c.held.clear();c.walker.Reset();c.automation.Reset();
            CoUninitialize();
        }
    } cleanup{turn};
    // Chat scopes use UIA's semantic control tree. RawView includes layout-only
    // wrappers; ControlView keeps source rows/actions without raising depth limits.
    // Fresh scope, attachment and mode proofs reuse this same retained walker.
    ComPtr<IUIAutomation2> automation;
    if(FAILED(CoCreateInstance(CLSID_CUIAutomation8,nullptr,CLSCTX_INPROC_SERVER,IID_PPV_ARGS(&automation)))
        || FAILED(automation->put_ConnectionTimeout(100)) || FAILED(automation->put_TransactionTimeout(100))
        || FAILED(automation.As(&c.automation)) || FAILED(automation->get_ControlViewWalker(&c.walker)))return emit("tree");
    if(FAILED(automation->ElementFromHandle(c.request.window,&turn.root)) || !turn.root)
        return emit("tree-query");
    if(!c.append(turn.root.Get(),0)) {
        const std::string failure=c.stage;
        if(failure=="query" || failure=="root-process-query" || failure=="descendant-process-query")
            return emit("tree-query");
        if(failure=="limit")return emit("tree-limit");
        if(failure=="limit-depth")return emit("tree-depth");
        if(failure=="limit-nodes")return emit("tree-nodes");
        if(failure=="limit-name")return emit("tree-name-limit");
        if(failure=="limit-text")return emit("tree-text-limit");
        if(failure=="limit-windows")return emit("tree-window-limit");
        if(failure=="limit-processes")return emit("tree-process-limit");
        if(failure=="duplicate")return emit("tree-duplicate");
        if(failure=="heading-property")return emit("tree-type");
        if(failure=="deadline")return emit("deadline");
        return emit("tree-pid");
    }
    c.observe_mode(turn.root.Get());c.observe_capability(turn.root.Get());
    if(std::string(c.mode_status)!="chat")return emit("mode");
    if(turn.mode!="input-replace-owned")return emit(turn.action());
    if(c.classic_controls.size()!=1 || c.start_controls.size()+c.send_controls.size()!=1)return emit("composer");
    turn.editor=c.classic_controls.front();
    turn.send=c.start_controls.empty()?c.send_controls.front():c.start_controls.front();
    turn.send_name=c.start_controls.empty()?L"Send message":L"Start task";
    if(!c.source_control(turn.editor.Get(),UIA_EditControlTypeId,L"Write your prompt to Claude",turn.original.editor_bounds,turn.original.editor_enabled)
        || !turn.original.editor_enabled
        || !c.source_control(turn.send.Get(),UIA_ButtonControlTypeId,turn.send_name,turn.original.start_bounds,turn.original.start_enabled))return emit("control");
    BOOL read_only=TRUE,password=TRUE,focusable=FALSE;
    if(FAILED(turn.editor->GetCurrentPatternAs(UIA_ValuePatternId,IID_PPV_ARGS(&turn.value))) || !turn.value
        || FAILED(turn.send->GetCurrentPatternAs(UIA_InvokePatternId,IID_PPV_ARGS(&turn.invoke))) || !turn.invoke
        || FAILED(turn.value->get_CurrentIsReadOnly(&read_only)) || read_only
        || FAILED(turn.editor->get_CurrentIsPassword(&password)) || password
        || FAILED(turn.editor->get_CurrentIsKeyboardFocusable(&focusable)) || !focusable)return emit("control");
    return emit(turn.input());
}
