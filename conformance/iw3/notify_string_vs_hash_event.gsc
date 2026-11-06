// question: is an event sent as a string the same event as its hash?
// unsupported: IW3 has no hash literals (#"..."): events are strings only; the game's lexer refuses '#' (bad token)
// expect compile error: bad token '#'
// vs t4: same (the form cannot exist in T4 either)

main()
{
	thread hash_waiter();
	thread string_waiter();
	wait 0.05;
	level notify("go");
	logprint("sent string");
	wait 0.05;
	level notify(#"back");
	logprint("sent hash");
}

hash_waiter()
{
	level waittill(#"go");
	logprint("hash waiter woke");
}

string_waiter()
{
	level waittill("back");
	logprint("string waiter woke");
}
