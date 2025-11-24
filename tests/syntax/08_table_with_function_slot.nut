// EXPECT: 0 errors
// Table containing function slots
local obj = {
    name = "test",
    getValue = function() {
        return 42;
    },
    count = 0
};
