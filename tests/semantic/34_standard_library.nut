// EXPECT: no errors
// The Squirrel standard libraries are registered on the root table by the host,
// so their functions resolve without being declared in any script.
srand(time());

local roll = rand() % 100;
local angle = atan2(sin(PI), cos(PI));
local size = ceil(fabs(-1.5)) + floor(sqrt(16.0)) + abs(-2);
local parts = split(strip("  a,b  "), ",");
local stamp = clock() + RAND_MAX;

if (startswith(format("%d", roll), "4"))
{
	::print(angle + size + stamp + parts.len());
}
