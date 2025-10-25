::Mod_ROTU.RNGSpawn <- {
	Day0 = ::MSU.Class.WeightedContainer(
		[
			[
					2,
					{
						Unit = ::Const.World.Spawn.Troops.LegendBasiliskDrone,
						Max = 1
					}
			],
			[
					4,
					{
						Unit = ::Const.World.Spawn.Troops.FaultFinder,
						Max = 1
					}
			],
			/////undeads
			[
				3,
				{
					Unit = ::Const.World.Spawn.Troops.LegendDemonHound,
					Max = 1
				}
			],
		]
	),
	Day50 = ::MSU.Class.WeightedContainer(
		[
			[
					4,
					{
						Unit = ::Const.World.Spawn.Troops.LegendBasiliskDrone,
						Max = 2
					}
			],
			[
					4,
					{
						Unit = ::Const.World.Spawn.Troops.ModDrasethisCatMinion,
						Max = 2
					}
			],
			[
				2,
				{
					Unit = ::Const.World.Spawn.Troops.Necromancer,
					Max = 1
				}
			],
			//barbarians
			[
				4,
				{
					Unit = ::Const.World.Spawn.Troops.BarbarianMarauder,
					Max = 10
				}
			]
		]
	),
	Day100 = ::MSU.Class.WeightedContainer(
		[
			[
					5,
					{
						Unit = ::Const.World.Spawn.Troops.LegendBasiliskDrone,
						Max = 7
					}
			]
		]
	)
};