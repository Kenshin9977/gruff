// question: is an event sent as a string the same event as its hash: does notify("go") wake waittill(#"go"), and notify(#"go") wake waittill("go")?
// fact: an event sent as a string is the same event as its hash: notify("go") wakes waittill(#"go") and notify(#"go") wakes waittill("go")
// vs t7: T7 events are strings only
// evidence: static: the Notify handler (0x2752160) turns a string event into its hash (0x2757b60) and uses a hash event as is (type 4); waits store hashes
// expect: f1 T0 sent string
// expect: f1 T1 hash waiter woke
// expect: f2 T0 sent hash
// expect: f2 T2 string waiter woke
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
