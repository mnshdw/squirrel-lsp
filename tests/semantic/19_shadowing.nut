// EXPECT: no errors
local x = 1;
if (true) {
    local x = 2;
    print(x);
}
print(x);
