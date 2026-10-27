// Pure source-bound selectors: never issue AX, clipboard, window or input calls.
#include "../claude_chat_turn.cpp"
#include <cassert>
bool claude_owned_mac_window(std::uint64_t, pid_t, CGRect) { return false; }
MacOwnedWindowState claude_mac_window_state(std::uint64_t, pid_t, CGRect) { return MacOwnedWindowState::Rejected; }
MacWindowObservation claude_mac_window_observation() { return {}; }
static Node fixture(int parent, const char* role, const char* label, const char* current = "") {
    return {nullptr, parent, role, label, current, CGRectZero, true};
}
int main(int argc, char** argv) {
    // Exercise the actual fixed-frame producer through Rust's owned transport
    // without issuing any AX, clipboard, window or input operation.
    if (argc == 2 && std::string(argv[1]) == "--claude-chat-turn") {
        std::string request_line;
        if (!std::getline(std::cin, request_line)) return 2;
        const bool timeout = request_line == "timeout";
        const auto rejection_index = request_line.rfind("input-rejection ",0) == 0
            ? request_line.substr(16) : "";
        const auto completion_index = request_line.rfind("completion ",0) == 0 ? request_line.substr(11) : "";
        std::fill(request_line.begin(), request_line.end(), '\0');
        if (timeout) {
            progress(NativeProgress::Window);
            progress(NativeProgress::Tree);
            std::this_thread::sleep_for(std::chrono::seconds(2));
            return 0;
        }
        if (!completion_index.empty()) {
            if (completion_index.size()!=1 || completion_index[0]<'0' || completion_index[0]>'5') return 3;
            const bool sent=completion_index=="1" || completion_index=="5", missing_finish=completion_index=="2";
            const ComposerShape shape{1,0,1};
            progress(NativeProgress::Window);
            progress(sent?NativeProgress::Press:NativeProgress::Composer);
            completion(std::cout,sent?"sent":"composer",100,missing_finish?nullptr:&shape);
            if (!missing_finish) {
                progress(NativeProgress::Finish);
                std::cout << (completion_index=="4"?"PRIVATE\n":sent?"turn sent\ncomposer 1 0 1\n":"turn composer\ncomposer 1 0 1\n") << std::flush;
            }
            if (completion_index=="3") return 3;
            if (completion_index=="4" || completion_index=="5") return 0;
            std::this_thread::sleep_for(std::chrono::seconds(2));
            return 0;
        }
        if (!rejection_index.empty()) {
            InputValueRejection rejected;
            PrivateInputValue value;
            const char* failure = nullptr;
            if (rejection_index == "0") {
                noted(false,"target-geometry",&failure);
                rejected = {"paste-focus-before",failure,"unmeasured"};
            } else if (rejection_index == "1") {
                assert(!private_input_value(kCFBooleanTrue,value.value,&failure));
                rejected = {"paste-value",failure,"unmeasured"};
            } else if (rejection_index == "2") {
                rejected = {"paste-focus-after",window_input_cause({"geometry"}),input_relation("prompt","prompt","")};
            } else if (rejection_index == "3") {
                assert(related_paste_value("foreign","prompt","",rejected)==PastedValue::Rejected);
            } else if (rejection_index == "4") {
                assert(std::string(input_readback_failure(true,false))=="input-value-mismatch");
                rejected = {"readback-value","value-other","other"};
            } else return 3;
            progress(NativeProgress::Window);
            progress(rejection_index == "4" ? NativeProgress::ReadbackCopy : NativeProgress::PastedValue);
            progress(NativeProgress::Finish);
            std::cout << "turn input-value-mismatch\ncomposer 1 1 0\n";
            write_input_rejection(std::cout,rejected);
            return 0;
        }
        for (unsigned site = 0; site < 16; ++site) progress(static_cast<NativeProgress>(site));
        std::cout << "turn sent\ncomposer 1 0 1\n";
        return 0;
    }
    struct FlushedFrames : std::stringbuf {
        unsigned flushes = 0;
        int sync() override { ++flushes; return 0; }
    } frames;
    std::ostream output(&frames);
    for (unsigned site = 0; site < 16; ++site) progress(static_cast<NativeProgress>(site), output);
    assert(frames.flushes == 16);
    assert(frames.str() == "progress window\nprogress tree\nprogress composer\nprogress initial-value\n"
        "progress focus\nprogress replace-select\nprogress prompt-clipboard\nprogress paste\n"
        "progress pasted-value\nprogress readback\nprogress readback-select\nprogress readback-copy\n"
        "progress collapse\nprogress send-tree\nprogress press\nprogress finish\n");
    FlushedFrames completed_frames;
    std::ostream completed_output(&completed_frames);
    const ComposerShape completed_shape{1,0,1};
    completion(completed_output,"composer",100,&completed_shape);
    completion(completed_output,"sent",0);
    assert(completed_frames.flushes==2);
    assert(completed_frames.str()=="completion composer 100 1 0 1\ncompletion sent 0\n");
    Request cutoff_fixture;
    cutoff_fixture.millis=15000;
    cutoff_fixture.deadline=Clock::now()+std::chrono::seconds(15);
    assert(completion_remaining(cutoff_fixture)==0);
    timespec cutoff_now{}; assert(clock_gettime(CLOCK_MONOTONIC,&cutoff_now)==0);
    cutoff_fixture.cutoff=static_cast<std::uint64_t>(cutoff_now.tv_sec)*1000+static_cast<std::uint64_t>(cutoff_now.tv_nsec)/1000000+100;
    assert(completion_remaining(cutoff_fixture)<=100);
    cutoff_fixture.deadline=Clock::now()+std::chrono::milliseconds(10);
    assert(completion_remaining(cutoff_fixture)<=10);
    cutoff_fixture.millis=1;
    assert(completion_remaining(cutoff_fixture)<=1);
    Tree composer_tree;
    composer_tree.nodes = {fixture(-1, "AXWindow", ""), fixture(0, "AXTextArea", "Write your prompt to Claude"),
        fixture(0, "AXButton", "Send message"), fixture(-1, "AXButton", "Start task")};
    const auto source_composer = composer_shape(composer_tree);
    assert(source_composer.editors == 1 && source_composer.start_tasks == 0 && source_composer.send_messages == 1);
    assert(composer_readiness(source_composer) == ComposerReadiness::Ready);
    assert(composer_readiness({0, 0, 1}) == ComposerReadiness::Pending);
    assert(composer_readiness({1, 0, 0}) == ComposerReadiness::Pending);
    assert(composer_readiness({0, 0, 0}) == ComposerReadiness::Pending);
    unsigned composer_reads = 0, composer_pauses = 0, input_actions = 0;
    assert(settle_composer_readiness([&] {
        const ComposerShape shape{1, 0, ++composer_reads == 1 ? 0U : 1U};
        return composer_readiness(shape);
    }, [] { return true; }, [&] { ++composer_pauses; }));
    ++input_actions;
    assert(composer_reads == 2 && composer_pauses == 1 && input_actions == 1);
    for (const ComposerShape shape : {ComposerShape{2, 0, 1}, ComposerShape{1, 2, 0},
            ComposerShape{1, 0, 2}, ComposerShape{1, 1, 1}}) {
        composer_reads = composer_pauses = input_actions = 0;
        if (settle_composer_readiness([&] { ++composer_reads; return composer_readiness(shape); },
                [] { return true; }, [&] { ++composer_pauses; })) ++input_actions;
        assert(composer_reads == 1 && composer_pauses == 0 && input_actions == 0);
    }
    composer_reads = composer_pauses = input_actions = 0;
    if (settle_composer_readiness([&] { ++composer_reads; return ComposerReadiness::Pending; },
            [&] { return composer_reads < 2; }, [&] { ++composer_pauses; })) ++input_actions;
    assert(composer_reads == 2 && composer_pauses == 1 && input_actions == 0);
    composer_reads = composer_pauses = 0;
    assert(!settle_composer_readiness([&] { ++composer_reads; return ComposerReadiness::Ready; },
        [&] { return composer_reads == 0; }, [&] { ++composer_pauses; }));
    assert(composer_reads == 1 && composer_pauses == 0);
    assert(!settle_composer_readiness([&] { ++composer_reads; return ComposerReadiness::Ready; },
        [] { return false; }, [] {}));
    assert(composer_reads == 1);
    unsigned window_queries = 0, window_pauses = 0;
    assert(settle_owned_window([&] {
        return ++window_queries == 1 ? MacOwnedWindowState::PendingFocus : MacOwnedWindowState::Ready;
    }, [] { return true; }, [&] { ++window_pauses; }));
    assert(window_queries == 2 && window_pauses == 1);
    window_queries = window_pauses = 0;
    assert(!settle_owned_window([&] { ++window_queries; return MacOwnedWindowState::Rejected; },
        [] { return true; }, [&] { ++window_pauses; }));
    assert(window_queries == 1 && window_pauses == 0);
    window_queries = window_pauses = 0;
    assert(!settle_owned_window([&] { ++window_queries; return MacOwnedWindowState::PendingFocus; },
        [&] { return window_queries < 2; }, [&] { ++window_pauses; }));
    assert(window_queries == 2 && window_pauses == 1);
    window_queries = window_pauses = 0;
    assert(!settle_owned_window([&] { ++window_queries; return MacOwnedWindowState::Ready; },
        [&] { return window_queries == 0; }, [&] { ++window_pauses; }));
    assert(window_queries == 1 && window_pauses == 0);
    window_queries = 0;
    assert(!settle_owned_window([&] { ++window_queries; return MacOwnedWindowState::Ready; },
        [] { return false; }, [] {}));
    assert(window_queries == 0);
    const MacWindowSafety safe{true,true,true,true,true,true,true};
    assert(std::string(mac_window_rejection(safe)) == "unmeasured");
    assert(classify_mac_owned_window(MacFocusRead::ReadyHeld,safe)==MacOwnedWindowState::Ready);
    assert(classify_mac_owned_window(MacFocusRead::CannotComplete,safe)==MacOwnedWindowState::PendingFocus);
    assert(classify_mac_owned_window(MacFocusRead::Rejected,safe)==MacOwnedWindowState::Rejected);
    for (unsigned missing=0; missing<7; ++missing) {
        auto unsafe=safe;
        switch(missing) {
            case 0: unsafe.foreground_same=false; break;
            case 1: unsafe.application_ready=false; break;
            case 2: unsafe.held_unique=false; break;
            case 3: unsafe.held_geometry_same=false; break;
            case 4: unsafe.held_normal=false; break;
            case 5: unsafe.display_contained=false; break;
            default: unsafe.stack_clear=false;
        }
        for (auto focus : {MacFocusRead::ReadyHeld, MacFocusRead::CannotComplete, MacFocusRead::Rejected})
            assert(classify_mac_owned_window(focus,unsafe)==MacOwnedWindowState::Rejected);
        const char* reasons[] = {"foreground", "application", "identity", "geometry", "layer", "display", "stack"};
        assert(std::string(mac_window_rejection(unsafe)) == reasons[missing]);
    }
    unsigned passive_reads=0, passive_pauses=0, actions=1;
    bool past_deadline=false;
    const auto settled_window = settle_composer_focus([&] {
        assert(actions==1); // The sole Select All is never replayed.
        const auto state=classify_mac_owned_window(++passive_reads==1
            ? MacFocusRead::CannotComplete : MacFocusRead::ReadyHeld,safe);
        return state==MacOwnedWindowState::Ready ? ComposerFocus::Focused : ComposerFocus::PendingIdentity;
    },[&] { return !past_deadline; },[&] { ++passive_pauses; });
    if (settled_window) ++actions;
    assert(passive_reads==2 && passive_pauses==1 && actions==2);
    passive_reads=passive_pauses=0; actions=1;
    assert(!settle_composer_focus([&] { ++passive_reads; return ComposerFocus::PendingIdentity; },
        [&] { return !past_deadline; },[&] { ++passive_pauses; past_deadline=true; }));
    assert(passive_reads==1 && passive_pauses==1 && actions==1);

    assert(pasted_value_state("","prompt",true,true)==PastedValue::Pending);
    assert(pasted_value_state("pro","prompt",true,true)==PastedValue::Pending);
    assert(pasted_value_state("prompt","prompt",true,true)==PastedValue::Ready);
    assert(pasted_value_state("foreign","prompt",true,true)==PastedValue::Rejected);
    assert(pasted_value_state("previous","prompt",true,true,"previous")==PastedValue::Pending);
    assert(pasted_value_state("other","prompt",true,true,"previous")==PastedValue::Rejected);
    assert(pasted_value_state("previous","prompt",true,false,"previous")==PastedValue::Rejected);
    assert(pasted_value_state("prompt","prompt",true,true,"previous")==PastedValue::Ready);
    assert(pasted_value_state("prompt","prompt",false,true)==PastedValue::Rejected);
    assert(pasted_value_state("prompt","prompt",true,false)==PastedValue::Rejected);
    assert(pasted_value_state(std::string(1025,'x'),"prompt",true,true)==PastedValue::Rejected);
    unsigned value_queries=0, readback_keys=0, value_pauses=0;
    if (settle_pasted_value([&] { assert(readback_keys==0); return ++value_queries==1
            ? PastedValue::Pending : PastedValue::Ready; }, [] { return true; },
            [&] { ++value_pauses; })) ++readback_keys;
    assert(value_queries==2 && value_pauses==1 && readback_keys==1);
    value_queries=readback_keys=value_pauses=0;
    unsigned value_deadline_checks=0;
    if (settle_pasted_value([&] { ++value_queries; return PastedValue::Ready; },
            [&] { return ++value_deadline_checks==1; }, [&] { ++value_pauses; })) ++readback_keys;
    assert(value_queries==1 && readback_keys==0 && value_pauses==0);
    value_queries=readback_keys=value_pauses=0;
    if (settle_pasted_value([&] { ++value_queries; return PastedValue::Rejected; },
            [] { return true; }, [&] { ++value_pauses; })) ++readback_keys;
    assert(value_queries==1 && readback_keys==0 && value_pauses==0);

    Tree scope_tree;
    scope_tree.nodes = {fixture(-1, "AXWindow", ""), fixture(0, "AXWebArea", ""),
        fixture(1, "AXStaticText", "Server error"), fixture(1, "AXButton", "Try again"),
        fixture(1, "AXButton", "View details"), fixture(1, "AXOther", "Message 3")};
    ScopeShape shape;
    assert(observe_scope_shape(scope_tree, shape, [] { return true; }));
    assert(std::string(shape.parent_kind) == "web-area" && std::string(shape.walk_end) == "boundary-web-area");
    assert(shape.counts[0] == 0 && shape.counts[1] == 1 && shape.counts[4] == 1);
    scope_tree.nodes.push_back(fixture(1, "AXStaticText", "Server error"));
    assert(!observe_scope_shape(scope_tree, shape, [] { return true; }));
    scope_tree.nodes.pop_back();
    assert(!observe_scope_shape(scope_tree, shape, [] { return false; }));

    Request diagnostic;
    for (const char* phase : {"deadline-window", "deadline-tree", "deadline-focus", "deadline-input",
            "deadline-press", "deadline-copy", "deadline-retry-ready", "deadline-retry"}) {
        diagnostic.deadline_phase=phase;
        assert(std::string(deadline_result(diagnostic,"control",true))==phase);
        assert(std::string(deadline_result(diagnostic,"action-uncertain",true))=="action-uncertain");
        assert(std::string(deadline_result(diagnostic,"copied",false))=="copied");
    }

    const auto parsed_budget = [](unsigned millis, const char* suffix = "") {
        std::istringstream frame("input 7 8 0 0 800 600 " + std::to_string(millis) + " 1000 9 70 - 73\n" + suffix);
        auto* original = std::cin.rdbuf(frame.rdbuf());
        std::cin.clear();
        Request parsed;
        const bool valid = request(parsed);
        std::cin.rdbuf(original);
        std::cin.clear();
        if (valid) {
            assert(parsed.millis == millis && parsed.cutoff == 1000);
            // A larger upfront relative cap never renews an exhausted absolute cutoff.
            parsed.owner = static_cast<unsigned>(getppid());
            parsed.cutoff = 1;
            assert(!within(parsed));
            parsed.cutoff = UINT64_MAX;
            parsed.deadline = Clock::now() - std::chrono::milliseconds(1);
            assert(!within(parsed));
        }
        return valid;
    };
    assert(parsed_budget(1));
    assert(parsed_budget(5000));
    assert(parsed_budget(15000));
    assert(!parsed_budget(0));
    assert(!parsed_budget(15001));
    assert(!parsed_budget(15000, "trailing"));

    assert(assistant_heading_label("generic description", "Claude responded: fixture") == "Claude responded: fixture");
    assert(assistant_heading_label("Claude responded: fixture", "") == "Claude responded: fixture");
    assert(assistant_heading_label("Claude responded: fixture", "Claude responded: other").empty());
    assert(assistant_heading_label("generic description", "other heading") == "generic description");

    assert(initial_input_failure(nullptr, false) == std::string("input-initial-unavailable"));
    assert(initial_input_failure(kCFBooleanTrue, false) == std::string("input-initial-unavailable"));
    assert(initial_input_failure(CFSTR(""), true) == std::string("input-initial-unavailable"));
    assert(initial_input_failure(CFSTR("private existing input"), false) == std::string("input-initial-nonempty"));
    assert(initial_input_failure(CFSTR(""), false) == nullptr);
    assert(input_readback_failure(false, false) == std::string("input-clipboard-mismatch"));
    assert(input_readback_failure(false, true) == std::string("input-clipboard-mismatch"));
    assert(input_readback_failure(true, false) == std::string("input-value-mismatch"));
    assert(input_readback_failure(true, true) == nullptr);
    assert(input_mode("input"));
    assert(input_mode("input-replace-owned"));
    assert(!input_mode("replace"));
    assert(initial_input_failure(CFSTR("existing"), false, true) == nullptr);
    assert(initial_input_failure(nullptr, false, true) == std::string("input-initial-unavailable"));
    assert(initial_input_failure(kCFBooleanTrue, false, true) == std::string("input-initial-unavailable"));
    assert(initial_input_failure(CFSTR("existing"), true, true) == std::string("input-initial-unavailable"));
    Node focus_control = fixture(0, "AXTextArea", "Write your prompt to Claude");
    focus_control.element = reinterpret_cast<AXUIElementRef>(CFSTR("retained-composer"));
    focus_control.bounds = CGRectMake(10, 20, 30, 40);
    assert(focused_identity(focus_control.element, focus_control, 7, 7, "AXTextArea", focus_control.bounds));
    assert(!focused_identity(reinterpret_cast<AXUIElementRef>(CFSTR("other")), focus_control, 7, 7, "AXTextArea", focus_control.bounds));
    assert(!focused_identity(focus_control.element, focus_control, 8, 7, "AXTextArea", focus_control.bounds));
    assert(!focused_identity(focus_control.element, focus_control, 7, 7, "AXButton", focus_control.bounds));
    assert(!focused_identity(focus_control.element, focus_control, 7, 7, "AXTextArea", CGRectZero));
    Request focus_request;
    focus_request.pid = 7;
    focus_request.bounds = CGRectMake(0, 0, 100, 100);
    FocusIdentity focused{focus_control.element, 7, "AXTextArea", focus_control.bounds};
    const char* focus_diagnostic = nullptr;
    assert(composer_focus_identity(focused,focus_control,focus_request,true,false,&focus_diagnostic)==ComposerFocus::Focused);
    assert(focus_diagnostic==nullptr);
    assert(composer_focus_identity(focused, focus_control, focus_request, true, false) == ComposerFocus::Focused);
    auto previous = focused;
    previous.element = reinterpret_cast<AXUIElementRef>(CFSTR("previous-copy-control"));
    previous.role = "AXButton";
    previous.bounds = CGRectMake(60, 60, 10, 10);
    assert(composer_focus_identity(previous,focus_control,focus_request,true,false,&focus_diagnostic)==ComposerFocus::PendingIdentity);
    assert(focus_diagnostic==nullptr); // A valid foreign focused element remains passive pending.
    assert(composer_focus_identity(previous, focus_control, focus_request, true, false) == ComposerFocus::PendingIdentity);
    assert(composer_focus_identity(previous, focus_control, focus_request, false, false) == ComposerFocus::Rejected);
    assert(composer_focus_identity(previous, focus_control, focus_request, true, true) == ComposerFocus::Rejected);
    auto replaced = focused;
    replaced.element = previous.element;
    assert(composer_focus_identity(replaced, focus_control, focus_request, true, false) == ComposerFocus::PendingIdentity);
    assert(composer_focus_identity(replaced, focus_control, focus_request, false, false) == ComposerFocus::Rejected);
    auto moved = focused;
    moved.bounds.origin.x += 1;
    assert(composer_focus_identity(moved, focus_control, focus_request, true, false) == ComposerFocus::Rejected);
    auto foreign = previous;
    foreign.pid = 8;
    assert(composer_focus_identity(foreign, focus_control, focus_request, true, false) == ComposerFocus::Rejected);
    auto outside = previous;
    outside.bounds.origin.x = 1000;
    assert(composer_focus_identity(outside, focus_control, focus_request, true, false) == ComposerFocus::Rejected);
    auto missing = previous;
    missing.element = nullptr;
    assert(composer_focus_identity(missing, focus_control, focus_request, true, false) == ComposerFocus::Rejected);
    for (const auto& test : std::vector<std::pair<FocusIdentity,const char*>>{
            {moved,"focused-geometry"}, {foreign,"focused-pid"}, {outside,"focused-geometry"}, {missing,"focused-element"}}) {
        focus_diagnostic=nullptr;
        assert(composer_focus_identity(test.first,focus_control,focus_request,true,false,&focus_diagnostic)==ComposerFocus::Rejected);
        assert(std::string(focus_diagnostic)==test.second);
    }
    auto role_changed=focused; role_changed.role="AXButton";
    focus_diagnostic=nullptr;
    assert(composer_focus_identity(role_changed,focus_control,focus_request,true,false,&focus_diagnostic)==ComposerFocus::Rejected);
    assert(std::string(focus_diagnostic)=="focused-role");
    focus_diagnostic=nullptr;
    assert(composer_focus_identity(focused,focus_control,focus_request,true,true,&focus_diagnostic)==ComposerFocus::Rejected);
    assert(std::string(focus_diagnostic)=="query-failed");
    for (const char* first : {"target-pid","target-bounds-unavailable","target-geometry","target-role",
            "focused-bounds-unavailable","query-failed"}) {
        focus_diagnostic=nullptr; unsigned skipped=0;
        assert(!(noted(false,first,&focus_diagnostic) && (++skipped!=0)));
        assert(skipped==0 && std::string(focus_diagnostic)==first);
        assert(composer_focus_identity(focused,focus_control,focus_request,false,false,&focus_diagnostic)==ComposerFocus::Rejected);
        assert(std::string(focus_diagnostic)==first); // Later classification cannot relabel the held first failure.
    }
    for (const char* window : {"foreground","application","focus","geometry","inventory","identity","layer","display","stack"})
        assert(std::string(window_input_cause({window}))==std::string("window-")+window);
    assert(std::string(window_input_cause({}))=="window-unmeasured");
    assert(std::string(input_relation("prompt","prompt","prompt"))=="expected");
    assert(std::string(input_relation("old","prompt","old"))=="initial");
    assert(std::string(input_relation("pro","prompt",""))=="prefix");
    assert(std::string(input_relation("other","prompt",""))=="other");
    InputValueRejection paste_rejected;
    assert(related_paste_value("pro","prompt","",paste_rejected)==PastedValue::Pending && paste_rejected.site==nullptr);
    assert(related_paste_value("prompt","prompt","",paste_rejected)==PastedValue::Ready && paste_rejected.site==nullptr);
    assert(related_paste_value("other","prompt","",paste_rejected)==PastedValue::Rejected);
    assert(std::string(paste_rejected.site)=="paste-relation" && std::string(paste_rejected.cause)=="value-other");
    for (const auto& test : std::vector<std::pair<CFTypeRef,const char*>>{{nullptr,"value-unavailable"},{kCFBooleanTrue,"value-type"}}) {
        PrivateInputValue value; focus_diagnostic=nullptr;
        assert(!private_input_value(test.first,value.value,&focus_diagnostic));
        assert(std::string(focus_diagnostic)==test.second);
    }
    const std::string oversized(1025,'x');
    auto large=CFStringCreateWithCString(nullptr,oversized.c_str(),kCFStringEncodingUTF8);
    PrivateInputValue bounded; focus_diagnostic=nullptr;
    assert(!private_input_value(large,bounded.value,&focus_diagnostic) && std::string(focus_diagnostic)=="value-size");
    CFRelease(large);
    const UniChar invalid_utf16[] = {0xd800};
    auto unencodable=CFStringCreateWithCharacters(nullptr,invalid_utf16,1);
    focus_diagnostic=nullptr;
    assert(!private_input_value(unencodable,bounded.value,&focus_diagnostic));
    assert(std::string(focus_diagnostic)=="value-encoding");
    CFRelease(unencodable);
    unsigned queries = 0, pauses = 0, keys = 0;
    assert(!settle_composer_focus([&] { ++queries; return ComposerFocus::Focused; },
        [] { return false; }, [&] { ++pauses; }));
    assert(queries == 0 && pauses == 0 && keys == 0);
    const bool settled = settle_composer_focus([&] {
        ++queries;
        assert(keys == 0);
        return composer_focus_identity(queries == 1 ? replaced : focused,
            focus_control, focus_request, true, false);
    }, [] { return true; }, [&] { ++pauses; });
    if (settled) ++keys;
    assert(queries == 2 && pauses == 1 && keys == 1);
    queries = pauses = keys = 0;
    const bool rejected = settle_composer_focus([&] {
        ++queries; return ComposerFocus::Rejected;
    }, [] { return true; }, [&] { ++pauses; });
    if (rejected) ++keys;
    assert(queries == 1 && pauses == 0 && keys == 0);
    unsigned checks = 0;
    assert(!settle_composer_focus([&] { ++queries; return ComposerFocus::Focused; },
        [&] { return ++checks == 1; }, [&] { ++pauses; }));
    assert(queries == 2 && pauses == 0 && keys == 0);
    checks = queries = pauses = 0;
    assert(!settle_composer_focus([&] { ++queries; return ComposerFocus::PendingIdentity; },
        [&] { return ++checks <= 2; }, [&] { ++pauses; }));
    assert(queries == 1 && pauses == 1);

    assert(tree_node_failure(33, 0, true, true, true, false) == std::string("tree-limit"));
    assert(tree_node_failure(0, 1024, true, true, true, false) == std::string("tree-limit"));
    assert(tree_node_failure(0, 0, false, true, true, false) == std::string("deadline"));
    assert(tree_node_failure(0, 0, true, false, true, false) == std::string("tree-pid"));
    assert(tree_node_failure(0, 0, true, true, false, false) == std::string("tree-pid"));
    assert(tree_node_failure(0, 0, true, true, true, true) == std::string("tree-duplicate"));
    assert(tree_node_failure(0, 0, true, true, true, false) == nullptr);
    Tree failed_tree;
    assert(!failed_tree.reject("tree-type"));
    assert(!failed_tree.reject("tree-query"));
    assert(std::string(failed_tree.failure) == "tree-type");
    ax_query_failed = true;
    for (const char* terminal : {"tree-pid", "tree-type", "tree-limit", "tree-window"}) {
        Tree terminal_tree;
        assert(!terminal_tree.reject(terminal));
        assert(std::string(terminal_tree.failure) == terminal);
    }
    ax_query_failed = false;
    for (const char* role : {"AXGroup", "AXHeading", "AXStaticText", "AXWindow"}) {
        auto plan = node_property_plan(role, "Chat");
        assert(!plan.control_metadata && !plan.current_token);
    }
    for (const char* name : {"Copy", "Try again", "Start task", "Send message"}) {
        auto plan = node_property_plan("AXButton", name);
        assert(plan.control_metadata && !plan.current_token);
    }
    auto mode_plan = node_property_plan("AXButton", "Chat");
    assert(mode_plan.control_metadata && mode_plan.current_token);
    auto editor_plan = node_property_plan("AXTextArea", "Write your prompt to Claude");
    assert(editor_plan.control_metadata && !editor_plan.current_token);
    Request request;
    request.prompt = "fresh user";
    request.marker = "fresh assistant";
    Tree tree;
    tree.nodes = {fixture(-1, "AXWindow", ""), fixture(0, "AXGroup", "Mode"),
        fixture(1, "AXButton", "Chat", "page"), fixture(0, "AXGroup", ""),
        fixture(3, "AXHeading", "Claude responded: fresh assistant"), fixture(3, "AXButton", "Copy")};
    for (auto& node : tree.nodes) {
        auto plan = node_property_plan(node.role, node.label);
        if (!plan.control_metadata) { node.enabled = false; node.bounds = CGRectZero; }
        if (!plan.current_token) node.current.clear();
    }
    assert(chat(tree));
    assert(scoped_control(tree, request, false) == 5);
    tree.nodes.push_back(fixture(3, "AXButton", "Copy"));
    assert(scoped_control(tree, request, false) == -1);
    tree.nodes.pop_back();
    tree.nodes[4].label = "fresh assistant";
    assert(scoped_control(tree, request, false) == -1);
    tree.nodes[4] = fixture(3, "AXStaticText", "NAN_CHECK_EXPECTED_FAILURE");
    tree.nodes[5].label = "Try again";
    assert(scoped_control(tree, request, true) == -1);
    tree.nodes.push_back(fixture(3, "AXStaticText", "fresh user"));
    assert(scoped_control(tree, request, true) == 5);
    tree.nodes[5].label = "Retry"; // A generic legacy Retry is not this source callback.
    assert(scoped_control(tree, request, true) == -1);
    tree.nodes[5].label = "Try again";
    tree.nodes.push_back(fixture(3, "AXStaticText", "fresh user"));
    assert(scoped_control(tree, request, true) == -1);
    const char* scope_failure = nullptr;
    assert(scoped_control(tree, request, true, &scope_failure) == -1);
    assert(std::string(scope_failure) == "scope-prompt-mismatch");
    tree.nodes.pop_back();
    tree.nodes[5].label = "unrelated";
    assert(scoped_control(tree, request, true, &scope_failure) == -1);
    assert(std::string(scope_failure) == "scope-control-absent");
    tree.nodes[5].label = "Try again";
    tree.nodes.push_back(fixture(3, "AXHeading", "other heading"));
    tree.nodes.push_back(fixture(3, "AXHeading", "second heading"));
    assert(scoped_control(tree, request, true, &scope_failure) == -1);
    assert(std::string(scope_failure) == "scope-heading-ambiguous");
    tree.nodes[4].label = "unrelated";
    assert(scoped_control(tree, request, true, &scope_failure) == -1);
    assert(std::string(scope_failure) == "scope-anchor-absent");
    tree.nodes[4].label = "NAN_CHECK_EXPECTED_FAILURE";
    tree.nodes.push_back(fixture(3, "AXStaticText", "NAN_CHECK_EXPECTED_FAILURE"));
    assert(scoped_control(tree, request, true, &scope_failure) == -1);
    assert(std::string(scope_failure) == "scope-anchor-ambiguous");
    Request details_request;
    details_request.prompt = "fresh user";
    details_request.bounds = CGRectMake(0, 0, 800, 600);
    Tree details_tree;
    details_tree.nodes = {fixture(-1, "AXWindow", ""), fixture(0, "AXGroup", ""),
        fixture(1, "AXStaticText", "Server error"), fixture(1, "AXStaticText", "fresh user"),
        fixture(1, "AXButton", "Try again"), fixture(1, "AXButton", "View details"),
        fixture(1, "AXHeading", "You said: fresh user")};
    for (auto& node : details_tree.nodes) { node.enabled = true; node.bounds = CGRectMake(20, 20, 30, 30); }
    assert(scoped_control(details_tree, details_request, true) == -1);
    assert(failure_details_control(details_tree, details_request) == 5);
    details_tree.nodes[4].label = "Retry";
    assert(failure_details_control(details_tree, details_request) == -1);
    details_tree.nodes[4].label = "Try again";
    details_tree.nodes.push_back(fixture(1, "AXButton", "Try again"));
    assert(failure_details_control(details_tree, details_request) == -1);
    details_tree.nodes.pop_back();
    details_tree.nodes[3].label = "different user";
    assert(failure_details_control(details_tree, details_request) == -1);
    details_tree.nodes[3].label = "fresh user";
    details_tree.nodes[3].parent = 0;
    assert(failure_details_control(details_tree, details_request) == -1);
    details_tree.nodes[3].parent = 1;
    for (const char* label : {"Server error", "Try again", "View details", "fresh user"}) {
        details_tree.nodes.push_back(fixture(1, label == std::string("Try again") || label == std::string("View details") ? "AXButton" : "AXStaticText", label));
        assert(failure_details_control(details_tree, details_request) == -1);
        details_tree.nodes.pop_back();
    }
    details_tree.nodes[5].enabled = false;
    assert(failure_details_control(details_tree, details_request) == -1);
    details_tree.nodes[5].enabled = true;
    details_tree.nodes[6].label = "You said: different user";
    assert(failure_details_control(details_tree, details_request) == -1);
    details_tree.nodes[6].label = "You said: fresh user";
    for (const char* role : {"AXWebArea", "AXScrollArea", "AXWindow"}) {
        details_tree.nodes[1].role = role;
        assert(failure_details_control(details_tree, details_request) == -1);
    }
    details_tree.nodes[1].role = "AXGroup";
    details_tree.nodes.push_back(fixture(1, "AXHeading", "Claude responded: previous turn"));
    assert(failure_details_control(details_tree, details_request) == -1);
    const char* details_failure = nullptr;
    assert(failure_details_control(details_tree, details_request, &details_failure) == -1);
    assert(std::string(details_failure) == "scope-heading-ambiguous");
    details_tree.nodes.pop_back();
    details_tree.nodes[2].label = "Different error";
    assert(failure_details_control(details_tree, details_request, &details_failure) == -1);
    assert(std::string(details_failure) == "scope-anchor-absent");
    details_tree.nodes[2].label = "Server error";
    details_tree.nodes.push_back(fixture(1,"AXStaticText","Server error"));
    assert(failure_details_control(details_tree, details_request, &details_failure) == -1);
    assert(std::string(details_failure) == "scope-anchor-ambiguous");
    details_tree.nodes.pop_back();
    details_tree.nodes[6].label = "You said: other user";
    assert(failure_details_control(details_tree, details_request, &details_failure) == -1);
    assert(std::string(details_failure) == "scope-prompt-mismatch");
    details_tree.nodes[6].label = "You said: fresh user";
    details_tree.nodes[5].label = "Other control";
    assert(failure_details_control(details_tree, details_request, &details_failure) == -1);
    assert(std::string(details_failure) == "scope-control-absent");
    details_tree.nodes[5].label = "View details";
    details_tree.nodes.push_back(fixture(1,"AXButton","View details"));
    assert(failure_details_control(details_tree, details_request, &details_failure) == -1);
    assert(std::string(details_failure) == "scope-control-ambiguous");
    Request row_request;
    row_request.prompt = "fresh user";
    Tree row_tree;
    row_tree.nodes = {fixture(-1, "AXWindow", ""), fixture(0, "AXGroup", "Message 5"),
        fixture(1, "AXHeading", "You said: fresh user"), fixture(1, "AXStaticText", "fresh user"),
        fixture(0, "AXGroup", "Message 6"), fixture(4, "AXStaticText", "Server error"),
        fixture(4, "AXButton", "Try again"), fixture(4, "AXButton", "View details"),
        fixture(4, "AXHeading", "Claude responded: optional summary")};
    RowShape rows{};
    assert(observe_row_shape(row_tree, row_request, rows, [] { return true; }));
    assert(rows[0] == 2 && rows[7] == 1 && rows[8] == 1 && rows[9] == 1 && rows[10] == 1 && rows[11] == 1);
    assert(failure_details_control(row_tree, row_request) == -1); // Observation never broadens actions.
    row_tree.nodes[4].label = "Message 8";
    assert(observe_row_shape(row_tree, row_request, rows, [] { return true; }) && rows[10] == 0);
    row_tree.nodes[4].label = "Message 6"; row_tree.nodes[4].parent = 1;
    assert(observe_row_shape(row_tree, row_request, rows, [] { return true; }) && rows[9] == 0);
    row_tree.nodes[4].parent = 0; row_tree.nodes[4].label = "Message 5";
    assert(observe_row_shape(row_tree, row_request, rows, [] { return true; }) && rows[12] == 1 && rows[10] == 0);
    row_tree.nodes[4].label = "Currently streaming message";
    assert(observe_row_shape(row_tree, row_request, rows, [] { return true; }) && rows[1] == 1 && rows[10] == 0);
    for (const char* label : {"Message 0", "Message 01", "Message 4097", "Message PRIVATE", "Message 5 extra"}) {
        assert(source_row_position(label) == 0);
    }
    unsigned budget_checks = 0;
    assert(!observe_row_shape(row_tree, row_request, rows, [&] { return ++budget_checks < 2; }));
    Tree response_scope;
    const char* response_failure = nullptr;
    assert(scoped_control(response_scope, request, false, &response_failure) == -1);
    assert(std::string(response_failure) == "scope-heading-absent");
    response_scope.nodes.push_back(fixture(-1, "AXHeading", "You said: fresh assistant"));
    assert(scoped_control(response_scope, request, false, &response_failure) == -1);
    assert(std::string(response_failure) == "scope-assistant-heading-absent");
    response_scope.nodes[0].label = "Claude responded: stale assistant";
    response_scope.nodes.push_back(fixture(0, "AXStaticText", "fresh assistant"));
    assert(scoped_control(response_scope, request, false, &response_failure) == -1);
    assert(std::string(response_failure) == "scope-marker-heading-absent");
    Request submission_request;
    submission_request.bounds = CGRectMake(0, 0, 800, 600);
    auto identity = reinterpret_cast<AXUIElementRef>(CFSTR("retained-control"));
    Node initial = {identity, 0, "AXButton", "Start task", "", CGRectMake(20, 20, 100, 40), false};
    Tree submission;
    submission.nodes.push_back(initial);
    CFRetain(identity);
    assert(enabled_submission(submission, initial, submission_request) == -1);
    submission.nodes[0].enabled = true;
    assert(enabled_submission(submission, initial, submission_request) == 0);
    submission.nodes.push_back(submission.nodes[0]);
    CFRetain(identity);
    assert(enabled_submission(submission, initial, submission_request) == -1);
    CFRelease(submission.nodes.back().element);
    submission.nodes.pop_back();
    CFRelease(submission.nodes[0].element);
    submission.nodes[0].element = reinterpret_cast<AXUIElementRef>(CFRetain(CFSTR("replacement")));
    assert(enabled_submission(submission, initial, submission_request) == -1);
    tree.nodes[2].enabled = false;
    assert(!chat(tree));
    tree.nodes[2].enabled = true;
    tree.nodes[2].current = "false";
    assert(!chat(tree));

    // Separate user/error rows plus prior verified headings: temporal authority
    // never turns the feed itself into an ErrorCard or changes legacy admission.
    Request temporal_request; temporal_request.prompt="failure nonce3";
    temporal_request.bounds=CGRectMake(0,0,800,600);
    Tree temporal;
    temporal.nodes={fixture(-1,"AXWindow",""),fixture(0,"AXWebArea",""),
        fixture(1,"AXGroup","feed"),fixture(2,"AXGroup","user"),
        fixture(3,"AXHeading","You said: failure nonce3"),fixture(3,"AXStaticText","failure nonce3"),
        fixture(2,"AXGroup","error"),fixture(6,"AXStaticText","Server error"),
        fixture(6,"AXButton","Try again"),fixture(6,"AXButton","View details"),
        fixture(2,"AXHeading","Claude responded: earlier verified response")};
    for(auto& node:temporal.nodes) node.bounds=CGRectMake(20,20,30,30);
    const char* temporal_failure="scope";
    assert(temporal_control(temporal,temporal_request,false,&temporal_failure)==8);
    assert(temporal_control(temporal,temporal_request,true,&temporal_failure)==9);
    assert(failure_details_control(temporal,temporal_request)==-1); // Legacy stays strict.
    temporal.nodes[8].parent=2; // Retry is not in the source ErrorCard group.
    assert(temporal_control(temporal,temporal_request,false,&temporal_failure)==-1);
    assert(std::string(temporal_failure)=="scope-heading-ambiguous");
    temporal.nodes[8].parent=6;
    temporal.nodes.push_back(fixture(6,"AXButton","Try again"));
    assert(temporal_control(temporal,temporal_request,false,&temporal_failure)==-1);
    temporal.nodes.pop_back();
    temporal.nodes.push_back(fixture(2,"AXStaticText","Server error"));
    assert(temporal_control(temporal,temporal_request,false,&temporal_failure)==-1);
    temporal.nodes.pop_back();
    temporal.nodes[4].label="You said: different prompt";
    assert(temporal_control(temporal,temporal_request,false,&temporal_failure)==-1);
    temporal.nodes[4].label="You said: failure nonce3";
    temporal.nodes[8].enabled=false;
    assert(temporal_control(temporal,temporal_request,false,&temporal_failure)==-1);
    temporal.nodes[8].enabled=true;
    Tree clean; clean.nodes={fixture(-1,"AXWindow",""),fixture(0,"AXTextArea","Write your prompt to Claude")};
    assert(clean_failure_input(clean,temporal_request));
    clean.nodes.push_back(fixture(1,"AXStaticText","failure nonce3"));
    assert(!clean_failure_input(clean,temporal_request));
    assert(clean_failure_input(clean,temporal_request,1));
    clean.nodes.push_back(fixture(0,"AXStaticText","failure nonce3"));
    assert(!clean_failure_input(clean,temporal_request,1)); clean.nodes.pop_back();
    clean.nodes.push_back(fixture(1,"AXStaticText","Server error"));
    assert(!clean_failure_input(clean,temporal_request,1)); clean.nodes.pop_back();
    clean.nodes.push_back(fixture(1,"AXButton","Try again"));
    assert(!clean_failure_input(clean,temporal_request,1)); clean.nodes.pop_back();
    clean.nodes.pop_back();
    clean.nodes.push_back(fixture(0,"AXStaticText","Server error"));
    assert(!clean_failure_input(clean,temporal_request)); clean.nodes.pop_back();
    clean.nodes.push_back(fixture(0,"AXButton","Try again"));
    assert(!clean_failure_input(clean,temporal_request)); clean.nodes.pop_back();
    clean.nodes.push_back(fixture(0,"AXHeading","You said: failure nonce3"));
    assert(!clean_failure_input(clean,temporal_request));
}
