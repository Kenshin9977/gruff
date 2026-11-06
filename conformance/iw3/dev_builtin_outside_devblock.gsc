// question: a dev-only builtin (println) called outside /# #/: refused at load, or what does the VM do with it?
// fact: the game's compiler accepts it; in retail (developer_script 0) it compiles the call away (the call is never made), with developer_script 1 the call runs
// expect: f0 T0 after
// evidence: static: the call emitter brackets a developer builtin with 0x50ac70 (with developer_script 0x15c2617 clear: records the code buffer position, mode 2) and the reset at 0x50af4c, which discards the call's code
// vs t4: same

main()
{
	println("x");
	logprint("after");
}
