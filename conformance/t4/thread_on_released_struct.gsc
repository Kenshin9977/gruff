// question: a thread whose self is a struct nobody references any more: does it keep running?
// expect: f0 T1 start
// expect: f1 T1 still running
// expect: f1 T1 self defined=1
// fact: a thread whose self struct is no longer referenced keeps running and self stays defined (the thread holds it)
// vs t7: same

main()
{
	s = spawnstruct();
	s thread worker();
	s = undefined;
}

worker()
{
	iprintln("start");
	wait 0.05;
	iprintln("still running");
	iprintln("self defined=" + isdefined(self));
}
