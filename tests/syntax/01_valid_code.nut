// EXPECT: 0 errors
function foo() {
    local x = 10;
    return x + 5;
}

local result = foo();
print(result);
