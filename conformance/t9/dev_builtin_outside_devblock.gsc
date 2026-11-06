// question: a dev-only builtin (println) called outside /# #/: what does the game's linker do at load?
// fact: a dev-only builtin (println) called outside /# #/ makes the load fail: the game's linker reports 'Dev only calls must be wrapped in a devblock' and ends in a fatal Sys_Error; acts by default wraps such a call in a devblock itself, so the test compiles with --no-devcall-inline
// vs t8: same
// expect error: Dev only calls must be wrapped in a devblock
// acts-flags: --no-devcall-inline
#namespace zz_test;

function main()
{
	println("x");
}
