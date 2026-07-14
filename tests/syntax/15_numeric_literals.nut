// EXPECT: 0 errors
// Floats can carry an exponent, with or without a fractional part.
local big = 1.0e9;
local small = 2.5E-3;
local exp = 1e9;
local trailing = 1.e9;
local plain = 1.5;
local int = 10;
local hex = 0xFF;
local negative = -1.0e-9;
local sum = big + small + exp + trailing + plain + int + hex + negative;
::print(sum);
