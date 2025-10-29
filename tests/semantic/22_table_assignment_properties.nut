// EXPECT: no errors
function getUnactivatedPerkTooltipHints() {
    return [
        {
            id = 3,
            type = "hint",
            icon = "ui/icons/damage_dealt.png",
            text = "[color=" + this.Const.UI.Color.PositiveValue + "]" + this.Math.round(this.getBonus() * 100) + "%[/color] Damage based on current Hitpoints and Fatigue"
        }
    ];
}
