#pragma once
#include <algorithm>
#include <cmath>
#include <vector>
struct MovePoint {double x,y;};
struct MoveRect {double x,y,w,h;};
struct MoveRow {bool metadata_valid,held,cursor,transparent;MoveRect bounds;};
struct MovePlan {bool measured=false,full_workarea_bounds_blocker=false,candidate=false;unsigned candidates_checked=0;MovePoint origin{};};
inline bool move_geometry(MoveRect r){return std::isfinite(r.x)&&std::isfinite(r.y)&&std::isfinite(r.w)&&std::isfinite(r.h)&&r.w>0&&r.h>0;}
inline bool move_contains(MoveRect r,MovePoint p){return p.x>=r.x&&p.y>=r.y&&p.x<=r.x+r.w&&p.y<=r.y+r.h;}
// PRIVATE candidate geometry only. A clear plan is not input/move authority.
// All non-held nontransparent window bounds participate, including windows behind the held
// one: no assumption about stacking after moving the owned window is made.
inline MovePlan codex_move_plan(MoveRect held,MoveRect work,const std::vector<MovePoint>& offsets,const std::vector<MoveRow>& rows){
 MovePlan out;
 if(!move_geometry(held)||!move_geometry(work)||held.w>work.w||held.h>work.h||offsets.empty()||offsets.size()>16||rows.empty()||rows.size()>1024)return out;
 unsigned held_count=0;
 for(const auto& row:rows){if(!row.metadata_valid||!move_geometry(row.bounds))return out;
  if(row.held){++held_count;if(row.bounds.x!=held.x||row.bounds.y!=held.y||row.bounds.w!=held.w||row.bounds.h!=held.h)return out;}}
 if(held_count!=1)return out;
 for(const auto& p:offsets)if(!std::isfinite(p.x)||!std::isfinite(p.y)||p.x<=0||p.y<=0||p.x>=held.w||p.y>=held.h)return out;
 out.measured=true;
 // Whole-area blocker is proved only in the currently ABOVE-held stack.
 // Behind-held rows may conservatively reject candidates but cannot justify
 // a claim that the current native point guard is blocked everywhere.
 for(const auto& row:rows){if(row.held)break;if(!row.cursor&&!row.transparent&&row.bounds.x<=work.x&&row.bounds.y<=work.y&&row.bounds.x+row.bounds.w>=work.x+work.w&&row.bounds.y+row.bounds.h>=work.y+work.h){out.full_workarea_bounds_blocker=true;return out;}}
 std::vector<MovePoint> origins;
 for(double fy:{0.,.5,1.})for(double fx:{0.,.5,1.}){MovePoint p{work.x+fx*(work.w-held.w),work.y+fy*(work.h-held.h)};
  if(p.x==held.x&&p.y==held.y)continue;
  if(std::none_of(origins.begin(),origins.end(),[p](MovePoint q){return p.x==q.x&&p.y==q.y;}))origins.push_back(p);}
 std::stable_sort(origins.begin(),origins.end(),[held](MovePoint a,MovePoint b){return std::hypot(a.x-held.x,a.y-held.y)<std::hypot(b.x-held.x,b.y-held.y);});
 for(auto origin:origins){++out.candidates_checked;bool clear=true;
  for(const auto& offset:offsets){MovePoint p{origin.x+offset.x,origin.y+offset.y};
   for(const auto& row:rows)if(!row.held&&!row.cursor&&!row.transparent&&move_contains(row.bounds,p)){clear=false;break;}
   if(!clear)break;}
  if(clear){out.candidate=true;out.origin=origin;return out;}}
 return out; // Finite candidate exhaustion does not prove the whole area blocked.
}
