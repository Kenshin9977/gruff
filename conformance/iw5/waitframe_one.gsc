// question: how many server frames do waitframe(1) and waitframe(2) take, against wait 0.05?
// unsupported: IW5 has no waitframe builtin: the compiler (gsc-tool) refuses the call
// expect compile error: couldn't determine function call type
// evidence: the compiler is gsc-tool (IW5 ships no compiler: its scripts are bytecode compiled offline)
// vs iw3: same (the form cannot exist in IW3 either)

main()
{
	logprint("start");
	waitframe(1);
	logprint("after waitframe 1");
	waitframe(2);
	logprint("after waitframe 2");
	wait 0.05;
	logprint("after wait 0.05");
}
