// EXPECT: Unknown
// Enum entries are declarations, not references to an outer variable, whether
// or not they are separated by commas. A value assigned to an entry is still
// resolved, so 'Unknown' is reported.
enum DayState {
	day night dawn
}

enum Direction {
	Up = 0,
	Down = 1,
	Missing = Unknown
}

::print(DayState.night);
::print(Direction.Up);
