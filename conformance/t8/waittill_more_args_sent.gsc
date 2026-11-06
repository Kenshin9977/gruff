// question: the notify sends three plain values after the event: what does the waiter's result struct hold?
// fact: plain values after the event are dropped: the waiter still wakes, with a struct that only has _notify
// vs t7: T7 binds the values to the waittill's names (extra values dropped)
// evidence: static: the Notify handler (0x2752160): when the value after the event is not an object, a new empty struct (AllocObject 0x2758a40) replaces it as the payload; the remaining values are popped and released
// expect: f1 T0 sent
// expect: f1 T1 struct=1 notify is e=1
#namespace zz_test;

function main()
{
	thread waiter();
	wait 0.05;
	level notify(#"e", 1, 2, 3);
	iprintln("sent");
}

function waiter()
{
	s = level waittill(#"e");
	iprintln("struct=" + isdefined(s) + " notify is e=" + (s._notify == #"e"));
}
