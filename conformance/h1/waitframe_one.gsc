// question: how many server frames does waitframe() take, against wait 0.05?
// fact: H1 has a waitframe statement (no argument): it waits one server frame, as wait 0.05 does
// evidence: the compiler is gsc-tool, which reads `waitframe()` as a statement with no argument (it refuses `waitframe(1)`) and emits the waitframe opcode; the VM runs it
// vs s1: same
// expect: f0 T0 start
// expect: f1 T0 after waitframe
// expect: f2 T0 after waitframe again
// expect: f3 T0 after wait 0.05

main()
{
	logprint("start");
	waitframe();
	logprint("after waitframe");
	waitframe();
	logprint("after waitframe again");
	wait 0.05;
	logprint("after wait 0.05");
}
