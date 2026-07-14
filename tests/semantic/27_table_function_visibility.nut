// EXPECT: no errors
// Functions defined in a table should be callable from sibling functions

my_class <- {
    function helper() {
        return 42;
    },

    function main() {
        local x = helper();  // helper should be visible here
        return x;
    }
};
