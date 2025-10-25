::Const.Tactical.ShadowParticles <- [
	{
		Delay = 0,
		Quantity = 50,
		LifeTimeQuantity = 0,
		SpawnRate = 10,
		Brushes = [
			"effect_lightning_01",
			"effect_lightning_02",
			"effect_lightning_03"
		],
		Stages = [
			{
				LifeTimeMin = 0.75,
				LifeTimeMax = 1.25,
				ColorMin = ::createColor("00000000"),
				ColorMax = ::createColor("00000000"),
				ScaleMin = 0.25,
				ScaleMax = 0.5,
				RotationMin = 0,
				RotationMax = 359,
				TorqueMin = -10,
				TorqueMax = 10,
				VelocityMin = 10,
				VelocityMax = 30,
				DirectionMin = ::createVec(-0.5, -0.5),
				DirectionMax = ::createVec(0.5, -0.5),
				SpawnOffsetMin = ::createVec(-50, 0),
				SpawnOffsetMax = ::createVec(50, 40),
				ForceMin = ::createVec(0, 0),
				ForceMax = ::createVec(0, 10),
				FlickerEffect = false
			},
			{
				LifeTimeMin = 4.0,
				LifeTimeMax = 6.0,
				ColorMin = ::createColor("0000002d"),
				ColorMax = ::createColor("0000002d"),
				ScaleMin = 0.5,
				ScaleMax = 1.0,
				VelocityMin = 10,
				VelocityMax = 30,
				DirectionMin = ::createVec(-0.400000006, -0.600000024),
				DirectionMax = ::createVec(0.400000006, -0.600000024),
				ForceMin = ::createVec(0, 0),
				ForceMax = ::createVec(0, 10),
				FlickerEffect = false
			},
			{
				LifeTimeMin = 0.5,
				LifeTimeMax = 1.0,
				ColorMin = ::createColor("00000000"),
				ColorMax = ::createColor("00000000"),
				ScaleMin = 0.5,
				ScaleMax = 1.0,
				VelocityMin = 10,
				VelocityMax = 30,
				ForceMin = ::createVec(0, 0),
				ForceMax = ::createVec(0, 10),
				FlickerEffect = false
			}
		]
	},
	{
		Delay = 0,
		Quantity = 50,
		LifeTimeQuantity = 0,
		SpawnRate = 8,
		Brushes = [
			"miasma_effect_02",
			"miasma_effect_03"
		],
		Stages = [
			{
				LifeTimeMin = 0.75,
				LifeTimeMax = 1.25,
				ColorMin = ::createColor("00000000"),
				ColorMax = ::createColor("00000000"),
				ScaleMin = 0.5,
				ScaleMax = 1.0,
				RotationMin = 0,
				RotationMax = 359,
				TorqueMin = -10,
				TorqueMax = 10,
				VelocityMin = 10,
				VelocityMax = 30,
				DirectionMin = ::createVec(-0.25, -0.25),
				DirectionMax = ::createVec(0.25, -0.25),
				SpawnOffsetMin = ::createVec(-50, 0),
				SpawnOffsetMax = ::createVec(50, 40),
				ForceMin = ::createVec(0, 0),
				ForceMax = ::createVec(0, 10),
				FlickerEffect = false
			},
			{
				LifeTimeMin = 4.0,
				LifeTimeMax = 6.0,
				ColorMin = ::createColor("00000030"),
				ColorMax = ::createColor("00000030"),
				ScaleMin = 0.75,
				ScaleMax = 1.25,
				VelocityMin = 10,
				VelocityMax = 30,
				DirectionMin = ::createVec(-0.200000003, -0.300000012),
				DirectionMax = ::createVec(0.200000003, -0.300000012),
				ForceMin = ::createVec(0, 0),
				ForceMax = ::createVec(0, 10),
				FlickerEffect = false
			},
			{
				LifeTimeMin = 0.5,
				LifeTimeMax = 1.0,
				ColorMin = ::createColor("00000000"),
				ColorMax = ::createColor("00000000"),
				ScaleMin = 0.75,
				ScaleMax = 1.25,
				VelocityMin = 10,
				VelocityMax = 30,
				ForceMin = ::createVec(0, 0),
				ForceMax = ::createVec(0, 10),
				FlickerEffect = false
			}
		]
	}
];
