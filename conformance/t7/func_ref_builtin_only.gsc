// question: `&spawnstruct` when no script function spawnstruct exists: a pointer to the builtin, or a load error?
// fact: a function reference falls back to the builtin when the namespace has no such function, and calling it runs the builtin
// claim: gscode-dialect-facts (disagrees): & references mean the script function
// expect: f0 T0 before
// expect: f0 T0 called, struct=1
#namespace zz_test;

function main()
{
	logprint("before");
	f = &spawnstruct;
	s = [[ f ]]();
	logprint("called, struct=" + isdefined(s));
}
