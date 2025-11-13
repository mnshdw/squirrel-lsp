::Mod_ROTU.ModHook.hook("scripts/skills/effects/insect_swarm_effect", function ( q ) {

	q.onAdded = @(__original) function()
	{
		local actor = this.getContainer().getActor();
		local crrd = this.getContainer().hasSkill("perk.crrangeddefense") ? this.Math.rand(1, 100) <= actor.getBaseProperties().RangedDefense : false;

		if (this.getContainer().getActor().getCurrentProperties().IsResistantToAnyStatuses && this.Math.rand(1, 100) <= 50 || crrd)
		{
			if (!this.getContainer().getActor().isHiddenToPlayer())
			{
				this.Tactical.EventLog.log(this.Const.UI.getColorizedEntityName(this.getContainer().getActor()) + " repels insects with his unnatural physiology");
			}

			this.removeSelf();
		}
		else
		{
			this.m.TurnsLeft = this.Math.max(1, 3 + this.getContainer().getActor().getCurrentProperties().NegativeStatusEffectDuration);
			this.Sound.play(this.m.SoundOnUse[this.Math.rand(0, this.m.SoundOnUse.len() - 1)], this.Const.Sound.Volume.Skill, this.getContainer().getActor().getPos());
			local actor = this.getContainer().getActor();
			this.addSprite(1, "bust_flies_01");
			this.addSprite(2, "bust_flies_02");
			this.addSprite(3, "bust_flies_03");
			this.addSprite(4, "bust_flies_04");
			this.addSprite(5, "bust_flies_05");
			this.addSprite(6, "bust_flies_06");
			this.addSprite(7, "bust_flies_07");
			this.addSprite(8, "bust_flies_08");
			this.addSprite(9, "bust_flies_09", true);
			this.addSprite(10, "bust_flies_10", true);
			this.addSprite(11, "bust_flies_04", true);
			this.addSprite(12, "bust_flies_05", true);
			this.addSprite(13, "bust_flies_06", true);
			this.addSprite(14, "bust_flies_08");
			this.addSprite(15, "bust_flies_05");
			actor.setSpriteOffset("insects_14", this.createVec(-20, 0));
			actor.setSpriteOffset("insects_15", this.createVec(10, 0));
		}
	}

	q.onUpdate = @(__original) function( _properties )
	{

		if (!this.getContainer().getActor().isPlayerControlled()) //Nerf the effect to -20% stats when it's on an enemy
		{
			_properties.MeleeSkillMult *= 0.8;
			_properties.RangedSkillMult *= 0.8;
			_properties.MeleeDefenseMult *= 0.8;
			_properties.RangedDefenseMult *= 0.8;
			_properties.InitiativeMult *= 0.8;
		}
		else	// Usual -50% when on a bro
		{
			_properties.MeleeSkillMult *= 0.5;
			_properties.RangedSkillMult *= 0.5;
			_properties.MeleeDefenseMult *= 0.5;
			_properties.RangedDefenseMult *= 0.5;
			_properties.InitiativeMult *= 0.5;
		}

		/*

		//If you want to keep the SSU resilient effect (important it only works with the ssu perk, not vanilla resilient) which reduces debuff effectiveness by 25% (multiplicative)

		local enemyMult = 80;
		local broMult = 50;
		if (this.getContainer().hasSkill("perk.crresilient"))
		{
			enemyMult = 85;  // 20% * 0.75 (reduced by 25%) = 15% or in other words it is reduced to 85%
			broMult = 63;	// 50% * 0.75 (reduced by 25%) ~ 37% or in other words it is reduced to 63%
		}
		if (!this.getContainer().getActor().isPlayerControlled())	//For enemies
		{
			_properties.MeleeSkillMult *= enemyMult * 0.01;
			_properties.RangedSkillMult *= enemyMult * 0.01;
			_properties.MeleeDefenseMult *= enemyMult * 0.01;
			_properties.RangedDefenseMult *= enemyMult * 0.01;
			_properties.InitiativeMult *= enemyMult * 0.01;
		}
		else 														//For bros
		{
			_properties.MeleeSkillMult *= broMult * 0.01;
			_properties.RangedSkillMult *= broMult * 0.01;
			_properties.MeleeDefenseMult *= broMult * 0.01;
			_properties.RangedDefenseMult *= broMult * 0.01;
			_properties.InitiativeMult *= broMult * 0.01;
		}*/

	}
});
