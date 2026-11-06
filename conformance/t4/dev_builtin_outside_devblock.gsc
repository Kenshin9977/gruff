// question: a dev-only builtin (println) called outside /# #/: refused at load, or what does the VM do with it?
// fact: the game's compiler accepts it; in retail (developer_script 0) it compiles the call away (the call is never made), with developer_script 1 the call runs
// expect: f0 T0 after
// evidence: static: the call emitter brackets a developer builtin with 0x682b30 / 0x682ba0: with scrVarPub.developer_script (0x3882b77) clear, 0x682b30 records the code buffer position and 0x682ba0 moves the buffer back to it, discarding the call's code
// vs t7: T7's linker refuses the script ('Dev only calls must be wrapped in a devblock')

main()
{
	println("x");
	iprintln("after");
}
