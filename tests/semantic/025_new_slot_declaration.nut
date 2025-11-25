// EXPECT: no errors
// The <- operator creates a new variable (slot) in the current scope

character_trait <- inherit("scripts/skills/skill", {
    function test() {
        return true;
    }
})

my_class <- {
    value = 42
}

// Using the declared variables should work
local x = character_trait;
local y = my_class;
