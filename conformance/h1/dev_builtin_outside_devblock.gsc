// question: a dev-only builtin (println) called outside /# #/: refused at load, or what does the VM do with it?
// fact: the call is accepted at load and runs: println's retail handler is an empty function (shared by the builtins this build strips), so nothing happens and the thread goes on
// evidence: the compiler is gsc-tool, which compiles the call (what H1's own compiler does with it is not measured); the game's loader and VM run it
// vs s1: same
// expect: f0 T0 after

main()
{
	println("x");
	logprint("after");
}
