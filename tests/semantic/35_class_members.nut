// EXPECT: typo
// Members of a class are in scope for its methods under their bare name. A class
// that extends a base we cannot see may also use inherited slots by their bare
// name, so nothing is reported inside it.
class Item
{
	price = 300;
	label = null;
	static Sound = "res://sounds/coin.wav";

	constructor( _label )
	{
		this.label = _label;
		price = 100;
	}

	function describe()
	{
		return label + Sound + price + typo;
	}
}

class Tent extends Item
{
	function use( _entity )
	{
		description = "You can sleep until next morning.";
		return price + inheritedFromBase;
	}
}
