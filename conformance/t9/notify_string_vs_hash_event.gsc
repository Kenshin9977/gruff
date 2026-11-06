// question: is an event sent as a string the same event as its hash: does notify("go") wake waittill(#"go"), and notify(#"go") wake waittill("go")?
// fact: an event sent as a string is the same event as its hash: notify("go") wakes waittill(#"go") and notify(#"go") wakes waittill("go")
// vs t8: T8 queues the woken waiters for the end of the notifier's run (T7 too); T9 runs each one inside the notify
// expect: f1 T1 hash waiter woke
// expect: f1 T0 sent string
// expect: f2 T2 string waiter woke
// expect: f2 T0 sent hash
#namespace zz_test;

function main()
{
	thread hash_waiter();
	thread string_waiter();
	wait 0.05;
	level notify("go");
	iprintln("sent string");
	wait 0.05;
	level notify(#"back");
	iprintln("sent hash");
}

function hash_waiter()
{
	level waittill(#"go");
	iprintln("hash waiter woke");
}

function string_waiter()
{
	level waittill("back");
	iprintln("string waiter woke");
}
