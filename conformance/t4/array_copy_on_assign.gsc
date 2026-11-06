// question: b = a for an array, then b is changed: does a change too (reference) or not (copy)?
// fact: b = a then a write through b leaves a unchanged: arrays are values (copied when written while shared)
// expect: f0 T0 a[0]=1 b[0]=2 a.size=1 b.size=2
// evidence: static: the array-ref evaluator (0x692850) tests the array variable's reference count (+4) at 0x692ab6: when another holder shares it, it drops its reference (0x690040), allocates a new array (0x68ff60) and copies the elements into it (0x690770) before the write (copy on write); the emulated run of this script reaches 0x68ff60 and 0x690770 between `b = a` and the first write through b
// vs t7: T7 shares the array (a[0]=2 b[0]=2 a.size=2 b.size=2)

main()
{
	a = [];
	a[0] = 1;
	b = a;
	b[0] = 2;
	b[1] = 3;
	iprintln("a[0]=" + a[0] + " b[0]=" + b[0] + " a.size=" + a.size + " b.size=" + b.size);
}
