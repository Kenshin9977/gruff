// Probe lint-class-cycle: two classes inheriting from each other.
#namespace zz_probe;

class ca : cb
{
}

class cb : ca
{
}

function autoexec main()
{
}
