#pragma once
#include <string>
#include <vector>
#include <optional>
#include <cstddef>

enum class UiaChatRole { Other, Heading, Button, Text, Group, Boundary };
// Containers retain topology, not duplicated aggregate names. Source action,
// heading and error text labels remain available to the exact turn scope.
inline bool uia_chat_retains_label(UiaChatRole role) {
    return role != UiaChatRole::Group && role != UiaChatRole::Boundary;
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
            if(node.role==UiaChatRole::Button && node.label==(retry?L"Retry":L"Copy")) {
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
                if(node.role==UiaChatRole::Button && node.label==L"Retry"){++retries;retry=static_cast<int>(index);}
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
