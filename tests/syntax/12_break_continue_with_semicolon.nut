// EXPECT: 0 errors
// The explicit ';' form must keep working.
function walk( _entities )
{
	foreach( entity in _entities )
	{
		if (entity == null) continue;

		switch (entity.direction)
		{
		case "up":
			entity.y -= entity.speed;
			break;

		default:
			break;
		}

		if (entity.isDone()) break;
	}
}

function gen()
{
	yield 1;
	yield 2
	yield
}
