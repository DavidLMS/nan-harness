#pragma once
#include <algorithm>
#include <ostream>

// Payload-free observations of the existing passive Send acquisition. These
// never select a control or authorize an action, including after a timeout.
struct UiaChatSendDiagnostic {
    enum class Phase { PasteCheck, PasteChecked, TreeScan, TreeScanned,
        CandidateCheck, TargetCheck, Release, Released };
    std::ostream& output;
    unsigned completed_scans{},pending_scans{};
    void mark(Phase phase) {
        const char* label="paste-check";
        switch(phase) {
            case Phase::PasteCheck: break;
            case Phase::PasteChecked: label="paste-checked";break;
            case Phase::TreeScan: label="tree-scan";break;
            case Phase::TreeScanned: label="tree-scanned";break;
            case Phase::CandidateCheck: label="candidate-check";break;
            case Phase::TargetCheck: label="target-check";break;
            case Phase::Release: label="release";break;
            case Phase::Released: label="released";break;
        }
        output<<"send-observation "<<label<<' '<<completed_scans<<' '<<pending_scans<<'\n'<<std::flush;
    }
    void scan_completed() {
        completed_scans=std::min(1024u,completed_scans+1);
        mark(Phase::TreeScanned);
    }
    void pending() {pending_scans=std::min(1024u,pending_scans+1);}
};
