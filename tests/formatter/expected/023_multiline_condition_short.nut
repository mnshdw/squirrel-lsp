function test(tile) {
	if (this.isFullBlessing()
		&& tile.Properties.Effect != null
		&& tile.Properties.Effect.Type == "fire")
	{
		return 1;
	}
}
