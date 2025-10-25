this.perk_legend_swagger <- this.inherit("scripts/skills/skill", {
	m = {
		ArmorAdded = 0,
		HelmetAdded = 0
	},

	function create() {
		::Const.Perks.setup(this.m, ::Legends.Perk.LegendSwagger);
		this.m.Type = this.Const.SkillType.Perk | this.Const.SkillType.StatusEffect;
		this.m.Order = this.Const.SkillOrder.Perk;
		this.m.IsActive = false;
		this.m.IsStacking = false;
		this.m.IsHidden = false;
	}

	function getTooltip() {
		local ret = [
			{
				id = 1,
				type = "title",
				text = this.getName()
			},
			{
				id = 2,
				type = "description",
				text = this.getDescription()
			}
		];
		local bonus = this.getBonus();
		if (bonus > 0) {
			ret.extend([
				{
					id = 10,
					type = "text",
					icon = "ui/icons/armor_body.png",
					text = "[color=" + this.Const.UI.Color.PositiveValue + "]+" + this.Math.floor(bonus) + "[/color] armor condition added at start of combat"
				},
				{
					id = 11,
					type = "text",
					icon = "ui/icons/armor_head.png",
					text = "[color=" + this.Const.UI.Color.PositiveValue + "]+" + this.Math.floor(bonus / 2) + "[/color] helmet condition added at start of combat"
				}
			]);
		} else {
			ret.push({
				id = 10,
				type = "text",
				icon = "ui/icons/warning.png",
				text = "This character is not wearing armor or their armor is too cheap to gain a bonus"

			});
		}

		return ret;
	}

	function getUnactivatedPerkTooltipHints() {
		local bonus = this.getBonus();
		local ret = [];
		if (this.Math.floor(bonus) > 0) {
			ret.extend([
				{
					id = 10,
					type = "text",
					icon = "ui/icons/armor_body.png",
					text = "[color=" + this.Const.UI.Color.PositiveValue + "]+" + this.Math.floor(bonus) + "[/color] armor condition added at start of combat"
				},
				{
					id = 11,
					type = "text",
					icon = "ui/icons/armor_head.png",
					text = "[color=" + this.Const.UI.Color.PositiveValue + "]+" + this.Math.floor(bonus / 2) + "[/color] helmet condition added at start of combat"
				}
			]);
		} else {
			ret.push({
				id = 10,
				type = "text",
				icon = "ui/icons/warning.png",
				text = "This character is not wearing armor or their armor is too cheap to gain a bonus"

			});
		}

		return ret;
	}

	function onCombatStarted() {
		this.m.ArmorAdded = this.Math.floor(this.getBonus());
		this.m.HelmetAdded = this.Math.floor(this.getBonus() / 2);
	}

	function onUpdate(_properties) {
		if (("State" in this.Tactical) && this.Tactical.State != null) {
			_properties.Armor[this.Const.BodyPart.Body] += this.m.ArmorAdded;
			_properties.Armor[this.Const.BodyPart.Head] += this.m.HelmetAdded;
			_properties.ArmorMax[this.Const.BodyPart.Body] += this.m.ArmorAdded;
			_properties.ArmorMax[this.Const.BodyPart.Head] += this.m.HelmetAdded;
		}
	}

	function onCombatFinished() {
		this.m.ArmorAdded = 0;
		this.m.HelmetAdded = 0;
	}

	function getBonus() {
		local actor = this.getContainer().getActor();
		local body = actor.getItems().getItemAtSlot(this.Const.ItemSlot.Body);
		local head = actor.getItems().getItemAtSlot(this.Const.ItemSlot.Head);
		local bodyvalue = 0;
		local headvalue = 0;

		if (body != null) {
			bodyvalue = bodyvalue + body.getValue();
		}

		if (head != null) {
			headvalue = headvalue + head.getValue();
		}

		local fat = actor.getItems().getStaminaModifier([::Const.ItemSlot.Body, ::Const.ItemSlot.Head,]);

		local gearvalue = bodyvalue + headvalue;
		return gearvalue * 0.002 * (1.0 + 0.01 * fat); //
	}

});
