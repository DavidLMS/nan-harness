// Synthetic geometry only. Never queries applications, windows or accessibility.
#import <CoreGraphics/CoreGraphics.h>
#include <cassert>
#include <limits>
bool mac_fit_rectangle(CGRect, CGRect, CGFloat, CGRect&);
bool mac_fit_resize_ack(CGRect, CGRect, CGRect);
int main() {
    CGRect fitted;
    const auto before = CGRectMake(10, 20, 1400, 900);
    const auto target = CGRectMake(0, 40, 1200, 760);
    assert(mac_fit_resize_ack(before, target, CGRectMake(10, 20, 1200, 760)));
    assert(!mac_fit_resize_ack(before, target, before));
    assert(!mac_fit_resize_ack(before, target, CGRectMake(11, 20, 1200, 760)));
    assert(!mac_fit_resize_ack(before, target, CGRectMake(10, 20, 1200, 759)));
    assert(mac_fit_rectangle(CGRectMake(10,20,1400,900), CGRectMake(0,0,1200,760),800,fitted));
    assert(CGRectEqualToRect(fitted,CGRectMake(0,40,1200,760)));
    assert(mac_fit_rectangle(CGRectMake(-1500,0,800,600),CGRectMake(-1600,100,1600,900),1000,fitted));
    assert(CGRectEqualToRect(fitted,CGRectMake(-1600,0,800,600)));
    assert(!mac_fit_rectangle(CGRectMake(0,0,800,600),CGRectMake(0,0,299,900),1000,fitted));
    assert(!mac_fit_rectangle(CGRectMake(0,0,800,199),CGRectMake(0,0,1600,900),1000,fitted));
    assert(!mac_fit_rectangle(CGRectMake(0,0,800,600),CGRectMake(0,0,1600,900),std::numeric_limits<double>::infinity(),fitted));
}
