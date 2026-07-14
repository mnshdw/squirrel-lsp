// EXPECT: 0 errors
// A newline terminates a statement in Squirrel, so 'break' and 'continue' do
// not require a trailing ';'.
function walk( _entities )
{
	foreach( entity in _entities )
	{
		if (entity == null)
			continue

		if (entity.isDone())
			break

		switch (entity.direction)
		{
		case "up":
			entity.y -= entity.speed
			break

		case "down":
			entity.y += entity.speed
			break

		default:
			break
		}

		while (entity.isBusy())
		{
			if (entity.isStuck()) break
			entity.step()
		}
	}
}
