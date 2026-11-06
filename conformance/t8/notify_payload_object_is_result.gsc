// question: a notify whose payload is an existing struct: is the waiter's result that same struct, with _notify written into it?
// fact: a notify whose payload is an object hands that same object to the waiter as the waittill result, and writes _notify into it
// vs t7: T7 waittill binds values to names; it returns nothing
// evidence: static: the Notify handler (0x2752160) keeps a pointer payload as is (type 1 at 0x2752252) and VM_Notify's result writer (0x27747c0) sets _notify on it
// expect: f1 T0 sent, obj._notify is e=1
// expect: f1 T1 same=1 v=5
#namespace zz_test;

function main()
{
	obj = spawnstruct();
	obj.v = 5;
	thread waiter(obj);
	wait 0.05;
	level notify(#"e", obj);
	iprintln("sent, obj._notify is e=" + (obj._notify === #"e"));
}

function waiter(obj)
{
	s = level waittill(#"e");
	iprintln("same=" + (s == obj) + " v=" + s.v);
}
