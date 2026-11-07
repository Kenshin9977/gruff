#include maps\_not_in_the_workspace;

main()
{
	helper_of_the_missing_file();   // fine: may be defined in the missing file
	r = randomint();                // B002: a builtin keeps its checks
}
