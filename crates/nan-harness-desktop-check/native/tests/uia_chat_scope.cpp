#include "../uia_chat_scope.hpp"
#include <cassert>
int main() {
    using R=UiaChatRole;
    std::vector<UiaChatScopeNode> nodes={{R::Other,L"",-1},{R::Other,L"",0},
        {R::Other,L"private prompt",1},{R::Button,L"Copy",1},
        {R::Other,L"",0},{R::Heading,L"Claude responded: marker",4},{R::Button,L"Copy",4}};
    auto result=uia_chat_scope(nodes,L"private prompt",L"marker",false);
    assert(result.control==6 && result.ancestor==4);
    nodes.push_back({R::Button,L"Copy",4});
    assert(uia_chat_scope(nodes,L"private prompt",L"marker",false).control<0);
    nodes.pop_back();nodes[5].label=L"User wrote: marker";
    assert(uia_chat_scope(nodes,L"private prompt",L"marker",false).control<0);
    nodes[5]={R::Other,L"NAN_CHECK_EXPECTED_FAILURE",4};nodes[6].label=L"Retry";
    assert(uia_chat_scope(nodes,L"private prompt",L"NAN_CHECK_EXPECTED_FAILURE",true).control<0);
    nodes.push_back({R::Other,L"private prompt",4});
    assert(uia_chat_scope(nodes,L"private prompt",L"NAN_CHECK_EXPECTED_FAILURE",true).control==6);
    nodes.push_back({R::Other,L"private prompt",4});
    assert(uia_chat_scope(nodes,L"private prompt",L"NAN_CHECK_EXPECTED_FAILURE",true).control<0);
    nodes.pop_back();nodes[4].parent=4;
    assert(uia_chat_scope(nodes,L"wrong prompt",L"NAN_CHECK_EXPECTED_FAILURE",true).control<0);
    assert(uia_chat_scope(std::vector<UiaChatScopeNode>(1025),L"prompt",L"marker",false).control<0);

    std::vector<UiaChatScopeNode> failed={{R::Boundary,L"",-1},{R::Group,L"",0},
        {R::Heading,L"You said: private failed prompt",1},
        {R::Text,L"private failed prompt",1},{R::Text,L"Server error",1},
        {R::Button,L"Retry",1},{R::Button,L"View details",1}};
    auto disclosure=uia_chat_failure_details(failed,L"private failed prompt");
    assert(disclosure.control==6 && disclosure.retry==5 && disclosure.user_heading==2);
    failed[2].label=L"You said: another prompt";
    assert(uia_chat_failure_details(failed,L"private failed prompt").control<0);
    failed[2].label=L"You said: private failed prompt";
    failed.push_back({R::Button,L"View details",1});
    assert(uia_chat_failure_details(failed,L"private failed prompt").control<0);
    failed.pop_back();failed[1].role=R::Boundary;
    assert(uia_chat_failure_details(failed,L"private failed prompt").control<0);
    failed[1].role=R::Group;failed[3].parent=0;
    assert(uia_chat_failure_details(failed,L"private failed prompt").control<0);
    failed[3].parent=1;failed.push_back({R::Heading,L"Claude responded: previous turn",1});
    assert(uia_chat_failure_details(failed,L"private failed prompt").control<0);
    failed.pop_back();failed.push_back({R::Text,L"Server error",1});
    assert(uia_chat_failure_details(failed,L"private failed prompt").control<0);

    failed.pop_back();
    failed.push_back({R::Heading,L"Claude responded: previous",1});
    assert(std::string(uia_chat_failure_details(failed,L"private failed prompt").failure)=="scope-heading-ambiguous");
    failed.pop_back();failed[6].label=L"Other control";
    assert(std::string(uia_chat_failure_details(failed,L"private failed prompt").failure)=="scope-control-absent");
    failed[6].label=L"View details";failed[2].label=L"You said: other";
    assert(std::string(uia_chat_failure_details(failed,L"private failed prompt").failure)=="scope-prompt-mismatch");
}
