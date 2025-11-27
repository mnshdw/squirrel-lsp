// EXPECT: 0 errors
// Assignment expression inside a for loop block
local x;
for (local i = 0; i < 10; ++i) {
    x = i;
}
