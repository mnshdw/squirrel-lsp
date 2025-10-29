::Const.Perks.TransformationTree <- {
	ID = "TransformationMagicTree",
	Name = "Transformation",
	Descriptions = [
		"transformation"
	],
	Tree = [
		[
			::Const.Perks.PerkDefs.RotuTransformationRatNimble,
            ::Const.Perks.PerkDefs.RotuTransformationTreeBf
		],
		[],
		[
            ::Const.Perks.PerkDefs.RotuTransformationRatAgile,
            ::Const.Perks.PerkDefs.RotuTransformationTreeSpike
        ],
		[
            ::Const.Perks.PerkDefs.RotuTransformationRatFast,
            ::Const.Perks.PerkDefs.RotuTransformationTreeRegenerative
        ],
		[],
		[],
		[
            ::Const.Perks.PerkDefs.RotuTransformationRatStacks,
            ::Const.Perks.PerkDefs.RotuTransformationTreeStacks
        ]
	]
};

// Making all the trees appear in alphabetical order because it's nice
::Const.Perks.MagicTrees.Tree = [
	::Const.Perks.AssassinMagicTree,
	::Const.Perks.BasicNecroMagicTree,
	::Const.Perks.BerserkerMagicTree,
	::Const.Perks.CaptainMagicTree,
	::Const.Perks.ConjurationMagicTree,
	::Const.Perks.DruidMagicTree,
	::Const.Perks.EvocationMagicTree,
	::Const.Perks.IllusionistMagicTree,
	::Const.Perks.PhilosophyMagicTree,
	::Const.Perks.RangerHuntMagicTree,
	::Const.Perks.SkeletonMagicTree,
	::Const.Perks.TransmutationMagicTree,
	::Const.Perks.ValaChantMagicTree,
	::Const.Perks.ValaTranceMagicTree,
	::Const.Perks.VampireMagicTree,
	::Const.Perks.WarlockMagicTree,
	::Const.Perks.ZombieMagicTree
];
