#include "../codex_move_plan.hpp"
#include <cassert>
#include <iostream>
int main(){MoveRect held{300,200,400,300},work{0,30,1000,700};std::vector<MovePoint> p{{200,150},{100,80}};std::vector<MoveRow> rows{{true,false,false,false,{490,340,20,20}},{true,true,false,false,held}};
 auto r=codex_move_plan(held,work,p,rows);assert(r.measured&&r.candidate&&!r.full_workarea_bounds_blocker&&r.candidates_checked<=9);
 rows[0].bounds=work;r=codex_move_plan(held,work,p,rows);assert(r.measured&&!r.candidate&&r.full_workarea_bounds_blocker);
 rows[0].transparent=true;assert(codex_move_plan(held,work,p,rows).candidate);
 rows[0].transparent=false;rows[0].cursor=true;assert(codex_move_plan(held,work,p,rows).candidate);
 rows[0].cursor=false;rows[0].metadata_valid=false;assert(!codex_move_plan(held,work,p,rows).measured);
 rows[0].metadata_valid=true;rows.push_back(rows[1]);assert(!codex_move_plan(held,work,p,rows).measured);rows.pop_back();
 assert(!codex_move_plan(held,work,{{0,100}},rows).measured);
 assert(!codex_move_plan(held,{0,0,200,200},p,rows).measured);
 rows[0].bounds={0,30,999,700};r=codex_move_plan(held,work,p,rows);assert(r.measured&&!r.candidate&&!r.full_workarea_bounds_blocker);
 // A behind-held full-area bound is not a currently blocking stack proof.
 rows[0].bounds=work;std::swap(rows[0],rows[1]);r=codex_move_plan(held,work,p,rows);assert(r.measured&&!r.candidate&&!r.full_workarea_bounds_blocker);
 std::cout<<"10 bounded prospective point-plan cases passed\n";
}
