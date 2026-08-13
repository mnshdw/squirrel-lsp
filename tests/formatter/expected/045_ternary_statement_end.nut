class Collision {
	function checkObject(entity, isPlayer) {
		local index = 999
		local tempDirection = (entity.knockedBack) ? entity.knockBackDirection : entity.direction

		foreach (object_index, object in objs[CURRENT_MAP]) {
			if (object != null) {
				checkTile(object)
			}
		}
		return index
	}

	function label(entity) {
		local name = (entity.isPlayer) ? entity.displayNameWithTitle : entity.displayNameShort
		// a comment between two statements is not part of either
		return name
	}
}
