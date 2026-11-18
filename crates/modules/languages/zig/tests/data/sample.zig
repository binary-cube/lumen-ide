//! A tiny stack.
const std = @import("std");

/// The most items a stack holds.
const MAX_ITEMS: usize = 16;

pub const Stack = struct {
    items: [MAX_ITEMS]i32 = undefined,
    len: usize = 0,

    pub fn push(self: *Stack, value: i32) error{Full}!void {
        if (self.len == MAX_ITEMS) return error.Full;
        self.items[self.len] = value;
        self.len += 1;
    }

    pub fn pop(self: *Stack) ?i32 {
        if (self.len == 0) return null;
        self.len -= 1;
        return self.items[self.len];
    }
};

test "push and pop" {
    var stack = Stack{};
    try stack.push(42);
    const top = stack.pop() orelse unreachable;
    std.debug.print("{d} {s}\n", .{ top, "ok" });
    const ok = true and 'x' != 0 and 3.5 > 1.0;
    _ = ok;
}
