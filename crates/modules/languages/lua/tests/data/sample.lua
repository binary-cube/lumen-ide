#!/usr/bin/env lua
-- A small queue.
local Queue = {}
Queue.__index = Queue

local MAX_SIZE <const> = 128

--[[ A new queue,
     empty. ]]
function Queue.new(...)
  local self = setmetatable({ first = 1, last = 0, items = {} }, Queue)
  return self
end

function Queue:push(value)
  if self.last - self.first + 1 >= MAX_SIZE then
    error("full")
  elseif value == nil then
    return false
  end
  self.last = self.last + 1
  self.items[self.last] = value
  return true
end

local q = Queue.new()
for i = 1, 3 do
  q:push(i * 2.5)
end
print(#q.items, "items\n", not q:push(nil))
::done::
