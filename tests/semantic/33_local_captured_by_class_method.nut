// EXPECT: no errors
// A class method closes over the enclosing scope, so a file-level local is
// resolvable from inside the class body.
local sleepSE = "res://sounds/sleep.wav";
local price = 300;

class Tent
{
	description = null;

	constructor()
	{
		this.description = price;
	}

	function use( _entity )
	{
		_entity.play(sleepSE);
		return true;
	}
}
