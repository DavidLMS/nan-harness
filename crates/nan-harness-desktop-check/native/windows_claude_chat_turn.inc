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
    std::wstring prompt,sentinel;
    UiaChatCapability original;
    const char* stage="request";
    ~ChatInput() {
        if(!prompt.empty())SecureZeroMemory(prompt.data(),prompt.size()*sizeof(wchar_t));
        if(!sentinel.empty())SecureZeroMemory(sentinel.data(),sentinel.size()*sizeof(wchar_t));
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
            && same && collection.mode_attached(editor.Get(),root.Get())
            && collection.mode_attached(send.Get(),root.Get()) && within();
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
        if(!owned() || !unique_source(editor.Get(),UIA_EditControlTypeId,L"Write your prompt to Claude")
            || !unique_source(send.Get(),UIA_ButtonControlTypeId,L"Start task"))return false;
        std::array<long,4> editor_bounds{},send_bounds{};bool editor_enabled=false,send_enabled=false;
        if(!collection.source_control(editor.Get(),UIA_EditControlTypeId,L"Write your prompt to Claude",editor_bounds,editor_enabled)
            || !editor_enabled || editor_bounds!=original.editor_bounds
            || !collection.source_control(send.Get(),UIA_ButtonControlTypeId,L"Start task",send_bounds,send_enabled)
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
    bool clipboard_write(const std::wstring& text) {
        if(!owned())return false;
        HWND owner=CreateWindowExW(0,L"STATIC",L"",0,0,0,0,0,HWND_MESSAGE,nullptr,GetModuleHandleW(nullptr),nullptr);
        if(!owner)return false;
        const auto bytes=(text.size()+1)*sizeof(wchar_t);
        HGLOBAL memory=GlobalAlloc(GMEM_MOVEABLE,bytes);bool transferred=false,success=false;
        if(memory) {
            void* locked=GlobalLock(memory);
            if(locked) {
                memcpy(locked,text.c_str(),bytes);GlobalUnlock(memory);
                if(OpenClipboard(owner)) {
                    if(EmptyClipboard() && within() && SetClipboardData(CF_UNICODETEXT,memory))transferred=true;
                    success=CloseClipboard() && transferred;
                }
            }
            if(!transferred) {
                if(void* buffer=GlobalLock(memory)){SecureZeroMemory(buffer,bytes);GlobalUnlock(memory);}
                GlobalFree(memory);
            }
        }
        DestroyWindow(owner);return success && owned();
    }
    std::optional<bool> clipboard_matches() {
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
                matched=valid && length==prompt.size() && std::equal(prompt.begin(),prompt.end(),text);
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
        const auto state=uia_chat_input_value(observed,prompt,SUCCEEDED(result)&&length<=1024,exact);
        if(!observed.empty())SecureZeroMemory(observed.data(),observed.size()*sizeof(wchar_t));
        return state;
    }
    const char* input() {
        if(!target())return "control";
        if(FAILED(editor->SetFocus()))return "input-focus-setting";
        if(!wait_focus())return "input-focused-identity";
        if(!key('A'))return "input-replace-select-key";
        if(!clipboard_write(prompt))return "input-prompt-clipboard";
        if(!key('V'))return "input-paste-key";
        if(!uia_chat_settle([&]{return pasted();},[&]{return within();},
            []{std::this_thread::sleep_for(std::chrono::milliseconds(20));}))return "input-value-mismatch";
        if(!clipboard_write(sentinel))return "input-sentinel-clipboard";
        if(!key('A'))return "input-readback-select-key";
        if(!key('C'))return "input-readback-copy-key";
        // Clipboard copy may settle, but neither Copy nor any key is repeated.
        while(within()) {
            if(focus()!=UiaChatInputValue::Ready)return "input-focused-identity";
            const auto copied=clipboard_matches();
            if(!copied)return "input-clipboard-mismatch";
            if(*copied)break;
            std::this_thread::sleep_for(std::chrono::milliseconds(20));
        }
        if(!within() || clipboard_matches()!=true || pasted()!=UiaChatInputValue::Ready)return "input-clipboard-mismatch";
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
    ChatInput turn;auto& c=turn.collection;
    std::uint64_t id=0;unsigned budget=0;std::string prompt_hex,sentinel_hex,extra;
    std::istringstream parser(*wire);
    const bool parsed=static_cast<bool>(parser>>id>>c.request.pid>>c.request.bounds.left
        >>c.request.bounds.top>>c.request.bounds.right>>c.request.bounds.bottom
        >>budget>>turn.cutoff>>turn.checker_pid>>prompt_hex>>sentinel_hex);
    if(!parsed || parser>>extra || !id || id>UINTPTR_MAX || !c.request.pid || !turn.checker_pid
        || budget<=50 || budget>15000 || turn.cutoff<=started || turn.cutoff-started>15000)return 2;
    auto prompt=uia_chat_hex(prompt_hex),sentinel=uia_chat_hex(sentinel_hex);
    SecureZeroMemory(wire->data(),wire->size());SecureZeroMemory(prompt_hex.data(),prompt_hex.size());
    SecureZeroMemory(sentinel_hex.data(),sentinel_hex.size());
    if(!prompt || !sentinel)return 2;
    const bool valid=chat_utf8(*prompt,turn.prompt) && chat_utf8(*sentinel,turn.sentinel);
    SecureZeroMemory(prompt->data(),prompt->size());SecureZeroMemory(sentinel->data(),sentinel->size());
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
            c.classic_controls.clear();c.start_controls.clear();c.held.clear();c.walker.Reset();c.automation.Reset();
            CoUninitialize();
        }
    } cleanup{turn};
    ComPtr<IUIAutomation2> automation;
    if(FAILED(CoCreateInstance(CLSID_CUIAutomation8,nullptr,CLSCTX_INPROC_SERVER,IID_PPV_ARGS(&automation)))
        || FAILED(automation->put_ConnectionTimeout(100)) || FAILED(automation->put_TransactionTimeout(100))
        || FAILED(automation.As(&c.automation)) || FAILED(automation->get_RawViewWalker(&c.walker)))return emit("tree");
    if(FAILED(automation->ElementFromHandle(c.request.window,&turn.root)) || !turn.root
        || !c.append(turn.root.Get(),0))return emit("tree");
    c.observe_mode(turn.root.Get());c.observe_capability(turn.root.Get());
    if(std::string(c.mode_status)!="chat")return emit("mode");
    if(std::string(c.capability_status)!="observed" || c.classic_controls.size()!=1 || c.start_controls.size()!=1
        || !c.capability.value_pattern || c.capability.read_only!=false || c.capability.password
        || !c.capability.keyboard_focusable || !c.capability.invoke_pattern)return emit("composer");
    turn.editor=c.classic_controls.front();turn.send=c.start_controls.front();turn.original=c.capability;
    if(FAILED(turn.editor->GetCurrentPatternAs(UIA_ValuePatternId,IID_PPV_ARGS(&turn.value))) || !turn.value
        || FAILED(turn.send->GetCurrentPatternAs(UIA_InvokePatternId,IID_PPV_ARGS(&turn.invoke))) || !turn.invoke)return emit("control");
    return emit(turn.input());
}
