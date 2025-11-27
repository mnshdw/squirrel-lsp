// EXPECT: no errors
// The function getContainer is defined in the parent skill class and should be accessible

skill <- {
  m = {
    Container = null
  },

  function getContainer() {
    return m.Container;
  }
};

::mods_hookBaseClass("skills/skill", function (o) {
});

this.perk_legend_ambidextrous <- this.inherit("scripts/skills/skill", {

  function onAdded() {
    local off = getContainer().getActor().getOffhandItem();
  }

});
