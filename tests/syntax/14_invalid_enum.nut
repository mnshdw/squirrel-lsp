// EXPECT: 1+ errors
// Entry separators are optional, but the enum itself still has to be well formed:
// this one is missing its name.
enum
{
	day
	night
}
