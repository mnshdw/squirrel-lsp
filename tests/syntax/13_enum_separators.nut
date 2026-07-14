// EXPECT: 0 errors
// Squirrel makes the ',' between enum entries optional, including after the
// last entry.
enum DayState
{
	day
	night
	dawn
}

enum Direction
{
	Up,
	Down,
	Left,
	Right,
}

enum Mixed
{
	First = 1
	Second = "two",
	Third
}

enum Single { Only }
