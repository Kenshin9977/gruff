// question: a thread whose self is a struct nobody references any more: does it keep running?
// fact: a thread whose self struct nobody references keeps running; self stays defined
// vs t7: same
// expect: f0 T1 start
// expect: f1 T1 still running
// expect: f1 T1 self defined=1
#namespace zz_test;

function main()
{
	s = spawnstruct();
	s thread worker();
	s = undefined;
}

function worker()
{
	iprintln("start");
	wait 0.05;
	iprintln("still running");
	iprintln("self defined=" + isdefined(self));
}
