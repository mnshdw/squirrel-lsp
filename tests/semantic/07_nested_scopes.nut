// EXPECT: no errors
function baz() {
    local outer = 1;
    if (true) {
        local inner = 2;
        print(outer);
        print(inner);
    }
}
