// question: `&spawnstruct` when no script function spawnstruct exists: a pointer to the builtin, or a load error?
// fact: &spawnstruct with no script function falls back to the builtin, and calling it works
// vs t7: same
// expect: f0 T0 before
// expect: f0 T0 called, struct=1
#namespace zz_test;

function main()
{
	iprintln("before");
	f = &spawnstruct;
	s = [[ f ]]();
	iprintln("called, struct=" + isdefined(s));
}
