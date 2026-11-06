// question: a dev-only builtin (println) called outside /# #/: what does the game's linker do at load?
// fact: a dev-only builtin (println) outside /# #/ is accepted by the game's compiler and the script runs on (println writes no trace line)
// vs t6: T6's linker refuses the script ('Dev only calls must be wrapped in a devblock')
// expect: f0 T0 before
// expect: f0 T0 after println

main()
{
	logprint("before");
	println("x");
	logprint("after println");
}
