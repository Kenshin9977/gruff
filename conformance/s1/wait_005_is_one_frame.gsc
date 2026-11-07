// question: how many server frames do wait 0.05, wait 0.1 and wait 0.15 take?
// expect: f0 T0 start
// expect: f1 T0 after 0.05
// expect: f3 T0 after 0.1
// expect: f6 T0 after 0.15
// fact: the VM time unit is one 50 ms frame: wait 0.05/0.1/0.15 take 1/2/3 frames
// vs iw6: same

main()
{
	logprint("start");
	wait 0.05;
	logprint("after 0.05");
	wait 0.1;
	logprint("after 0.1");
	wait 0.15;
	logprint("after 0.15");
}
