#include "../uia_chat_send_diagnostic.hpp"
#include <cassert>
#include <iostream>
#include <sstream>
#include <string>
int main(int argc,char** argv) {
    std::ostringstream output;
    UiaChatSendDiagnostic observation{output};
    using Phase=UiaChatSendDiagnostic::Phase;
    observation.mark(Phase::PasteCheck);
    observation.mark(Phase::PasteChecked);
    observation.mark(Phase::TreeScan);
    observation.scan_completed();
    observation.pending();
    observation.mark(Phase::Release);
    observation.mark(Phase::Released);
    observation.mark(Phase::PasteCheck);
    observation.mark(Phase::PasteChecked);
    observation.mark(Phase::TreeScan);
    observation.scan_completed();
    observation.mark(Phase::CandidateCheck);
    observation.mark(Phase::TargetCheck);
    observation.mark(Phase::Release);
    observation.mark(Phase::Released);
    assert(observation.completed_scans==2 && observation.pending_scans==1);
    assert(output.str().find("send-observation tree-scan 1 1\n")!=std::string::npos);
    assert(output.str().find("send-observation target-check 2 1\n")!=std::string::npos);
    if(argc==2 && std::string(argv[1])=="--wire")std::cout<<output.str();
    for(unsigned count=0;count<2048;++count){observation.scan_completed();observation.pending();}
    assert(observation.completed_scans==1024 && observation.pending_scans==1024);
}
