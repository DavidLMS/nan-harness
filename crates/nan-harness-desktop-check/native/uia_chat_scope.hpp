#pragma once
#include <string>
#include <vector>
#include <optional>
#include <cstddef>
#include <algorithm>

enum class UiaChatRole { Other, Heading, Button, Text, Group, Boundary };
// Both public labels dispatch the pinned renderer's ordinary onRetry handler.
// Return only a closed label; arbitrary accessible names never become actions.
inline const wchar_t* uia_chat_retry_name(const std::wstring& label) {
    if(label==L"Retry")return L"Retry";
    if(label==L"Try again")return L"Try again";
    return nullptr;
}
// Containers retain topology, not duplicated aggregate names. Exact public
// recovery labels remain observable across roles without granting action authority.
inline bool uia_chat_retains_label(UiaChatRole role, const std::wstring& label) {
    return (role != UiaChatRole::Group && role != UiaChatRole::Boundary)
        || uia_chat_retry_name(label) || label == L"View details";
}
struct UiaChatScopeNode { UiaChatRole role{}; std::wstring label; int parent=-1; };
struct UiaChatScope { int control=-1,anchor=-1,ancestor=-1,user_heading=-1,retry=-1; const char* failure="scope"; };
inline bool uia_chat_descendant(const std::vector<UiaChatScopeNode>& nodes,std::size_t child,int ancestor) {
    for(unsigned depth=0;child<nodes.size() && depth<=32;++depth) {
        const int parent=nodes[child].parent;
        if(parent==ancestor)return true;
        if(parent<0 || static_cast<std::size_t>(parent)>=child)return false;
        child=static_cast<std::size_t>(parent);
    }
    return false;
}
inline UiaChatScope uia_chat_scope(const std::vector<UiaChatScopeNode>& nodes,
        const std::wstring& prompt,const std::wstring& marker,bool retry) {
    UiaChatScope result;
    if(nodes.size()>1024 || marker.empty())return result;
    for(std::size_t index=0;index<nodes.size();++index) {
        const auto& node=nodes[index];
        const bool anchor=retry ? node.label.find(marker)!=std::wstring::npos
            : node.role==UiaChatRole::Heading && node.label.rfind(L"Claude responded:",0)==0
                && node.label.find(marker)!=std::wstring::npos;
        if(anchor) {
            if(result.anchor>=0){result.failure="scope-anchor-ambiguous";return result;}
            result.anchor=static_cast<int>(index);
        }
    }
    if(result.anchor<0){result.failure="scope-anchor-absent";return result;}
    bool ambiguous=false,mismatch=false;
    int ancestor=nodes[result.anchor].parent;
    for(unsigned depth=0;ancestor>=0 && depth<6;++depth) {
        if(static_cast<std::size_t>(ancestor)>=nodes.size()){result.failure="scope";return result;}
        if(nodes[ancestor].parent<0)break;
        unsigned headings=0,controls=0,prompts=0;int control=-1;
        for(std::size_t index=0;index<nodes.size();++index) {
            if(!uia_chat_descendant(nodes,index,ancestor))continue;
            const auto& node=nodes[index];
            headings+=node.role==UiaChatRole::Heading;
            prompts+=node.label==prompt;
            if(node.role==UiaChatRole::Button && (retry?uia_chat_retry_name(node.label)!=nullptr:node.label==L"Copy")) {
                ++controls;control=static_cast<int>(index);
            }
        }
        if(controls>1)ambiguous=true;
        if(controls==1) {
            if(headings>1){result.failure="scope-heading-ambiguous";return result;}
            if((retry && prompts!=1) || (!retry && prompts!=0))mismatch=true;
            else {result.control=control;result.ancestor=ancestor;return result;}
        }
        const int next=nodes[ancestor].parent;
        if(next>=ancestor){result.failure="scope";return result;}
        ancestor=next;
    }
    result.failure=mismatch?"scope-prompt-mismatch":ambiguous?"scope-control-ambiguous":"scope-control-absent";
    return result;
}

inline UiaChatScope uia_chat_failure_details(const std::vector<UiaChatScopeNode>& nodes,
                                            const std::wstring& prompt) {
    UiaChatScope result;
    if(nodes.size()>1024 || prompt.empty())return result;
    for(std::size_t index=0;index<nodes.size();++index) {
        if(nodes[index].role!=UiaChatRole::Text || nodes[index].label!=L"Server error")continue;
        if(result.anchor>=0){result.failure="scope-anchor-ambiguous";return result;}
        result.anchor=static_cast<int>(index);
    }
    if(result.anchor<0){result.failure="scope-anchor-absent";return result;}
    int ancestor=nodes[result.anchor].parent;
    for(unsigned depth=0;ancestor>=0 && depth<6;++depth) {
        if(static_cast<std::size_t>(ancestor)>=nodes.size() || nodes[ancestor].parent<0
            || nodes[ancestor].role==UiaChatRole::Boundary)break;
        if(nodes[ancestor].role==UiaChatRole::Group) {
            unsigned users=0,headings=0,prompts=0,retries=0,details=0;int control=-1,user=-1,retry=-1;
            for(std::size_t index=0;index<nodes.size();++index) {
                if(!uia_chat_descendant(nodes,index,ancestor))continue;
                const auto& node=nodes[index];
                headings+=node.role==UiaChatRole::Heading;
                if(node.role==UiaChatRole::Heading && node.label==L"You said: "+prompt){++users;user=static_cast<int>(index);}
                prompts+=node.role==UiaChatRole::Text && node.label==prompt;
                if(node.role==UiaChatRole::Button && uia_chat_retry_name(node.label)!=nullptr){++retries;retry=static_cast<int>(index);}
                if(node.role==UiaChatRole::Button && node.label==L"View details"){++details;control=static_cast<int>(index);}
            }
            if(users==1 && headings==1 && prompts==1 && retries==1 && details==1) {
                result.control=control;result.ancestor=ancestor;result.user_heading=user;result.retry=retry;return result;
            }
            if(users>1 || headings>1 || prompts>1 || retries>1 || details>1){
                result.failure=headings>1?"scope-heading-ambiguous":prompts>1?"scope-prompt-mismatch":"scope-control-ambiguous";
                return result;
            }
            if(users==1 && headings==1 && prompts==1 && (retries==0 || details==0))result.failure="scope-control-absent";
        }
        const int next=nodes[ancestor].parent;
        if(next>=ancestor)return result;
        ancestor=next;
    }
    if(std::string(result.failure)=="scope")result.failure="scope-prompt-mismatch";
    return result;
}

// Closed passive counts from the already bounded, owned ControlView collection.
// Group counts describe source shape; they never admit a disclosure or Retry.
struct UiaChatFailureCounts {
    unsigned server_errors=0,user_headings=0,prompt_texts=0,retries=0,details=0,
        prompt_groups=0,group_retries=0,group_details=0,retry_labels=0,details_labels=0;
    unsigned buttons=0,unnamed_buttons=0,group_buttons=0,group_unnamed_buttons=0,
        error_group_buttons=0,error_group_unnamed_buttons=0;
};
inline UiaChatFailureCounts uia_chat_failure_counts(const std::vector<UiaChatScopeNode>& nodes,
                                                   const std::wstring& prompt) {
    UiaChatFailureCounts result;
    if(nodes.size()>1024 || prompt.empty())return result;
    for(const auto& node:nodes) {
        result.buttons+=node.role==UiaChatRole::Button;
        result.unnamed_buttons+=node.role==UiaChatRole::Button && node.label.empty();
        result.server_errors+=node.role==UiaChatRole::Text && node.label==L"Server error";
        result.user_headings+=node.role==UiaChatRole::Heading && node.label==L"You said: "+prompt;
        result.prompt_texts+=node.role==UiaChatRole::Text && node.label==prompt;
        result.retry_labels+=uia_chat_retry_name(node.label)!=nullptr;
        result.details_labels+=node.label==L"View details";
        result.retries+=node.role==UiaChatRole::Button && uia_chat_retry_name(node.label)!=nullptr;
        result.details+=node.role==UiaChatRole::Button && node.label==L"View details";
    }
    for(std::size_t group=0;group<nodes.size();++group) {
        if(nodes[group].role!=UiaChatRole::Group)continue;
        unsigned headings=0,users=0,prompts=0,retries=0,details=0,buttons=0,unnamed=0,errors=0;
        for(std::size_t index=0;index<nodes.size();++index) {
            if(!uia_chat_descendant(nodes,index,static_cast<int>(group)))continue;
            const auto& node=nodes[index];
            buttons+=node.role==UiaChatRole::Button;
            unnamed+=node.role==UiaChatRole::Button && node.label.empty();
            errors+=node.role==UiaChatRole::Text && node.label==L"Server error";
            headings+=node.role==UiaChatRole::Heading;
            users+=node.role==UiaChatRole::Heading && node.label==L"You said: "+prompt;
            prompts+=node.role==UiaChatRole::Text && node.label==prompt;
            retries+=node.role==UiaChatRole::Button && uia_chat_retry_name(node.label)!=nullptr;
            details+=node.role==UiaChatRole::Button && node.label==L"View details";
        }
        if(headings==1 && users==1 && prompts==1) {
            ++result.prompt_groups;
            result.group_buttons=std::max(result.group_buttons,buttons);
            result.group_unnamed_buttons=std::max(result.group_unnamed_buttons,unnamed);
            if(errors==1) {
                result.error_group_buttons=std::max(result.error_group_buttons,buttons);
                result.error_group_unnamed_buttons=std::max(result.error_group_unnamed_buttons,unnamed);
            }
            // Maxima avoid double counting controls across nested groups.
            result.group_retries=std::max(result.group_retries,retries);
            result.group_details=std::max(result.group_details,details);
        }
    }
    return result;
}
