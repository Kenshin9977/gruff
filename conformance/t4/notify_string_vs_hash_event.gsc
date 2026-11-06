// question: is an event sent as a string the same event as its hash?
// unsupported: T4 has no hash literals (#"..."): events are strings only; the game's lexer refuses '#' (bad token)
// expect compile error: bad token '#'
// vs t7: T7 events are strings only too (T8: a string event is the same event as its hash)

main()
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

hash_waiter()
{
	level waittill(#"go");
	iprintln("hash waiter woke");
}

string_waiter()
{
	level waittill("back");
	iprintln("string waiter woke");
}
