package tqlmate

class CliTest extends munit.FunSuite:
  test("parses status flags"):
    val args = Cli.parse(List("status", "--exit-code", "--quiet")).toOption.get
    assertEquals(args.cmd, Cmd.Status(exitCode = true, quiet = true))

  test("parses down as rollback"):
    val args = Cli.parse(List("down")).toOption.get
    assertEquals(args.cmd, Cmd.Rollback)

  test("parses new with global url"):
    val args = Cli.parse(List("-u", "typedb://admin:password@localhost/db", "new", "add_person")).toOption.get
    assertEquals(args.global.url, Some("typedb://admin:password@localhost/db"))
    assertEquals(args.cmd, Cmd.New("add_person"))

  test("parses wait timeout"):
    val args = Cli.parse(List("wait", "--timeout", "15")).toOption.get
    assertEquals(args.cmd, Cmd.Wait(15))

  test("help on missing command"):
    assert(Cli.parse(Nil).isLeft)
