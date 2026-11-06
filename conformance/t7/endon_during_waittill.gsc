// question: an endon fired while the thread waits on another event: is the thread killed?
// expect: f0 T1 waiting
// expect: f1 T0 sent stop
// expect: f1 T0 sent go
// fact: an endon notify kills a thread blocked in waittill on another event
#namespace zz_test;

function main()
{
	thread waiter();
	wait 0.05;
	level notify("stop");
	logprint("sent stop");
	level notify("go");
	logprint("sent go");
}

function waiter()
{
	level endon("stop");
	logprint("waiting");
	level waittill("go");
	logprint("woke (should not happen)");
}
