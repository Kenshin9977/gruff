// question: the notify sends three values, waittill binds one: the extra values are dropped without error?
// fact: values sent beyond the waittill's names are dropped without error
// expect: f1 T0 sent
// expect: f1 T1 a=1
#namespace zz_test;

function main()
{
	thread waiter();
	wait 0.05;
	level notify("e", 1, 2, 3);
	logprint("sent");
}

function waiter()
{
	level waittill("e", a);
	logprint("a=" + a);
}
