package tqlmate

import java.nio.file.{Files, Path}
import scala.concurrent.duration.*
import scala.util.Try

class IntegrationTest extends munit.FunSuite:
  override def munitIgnore: Boolean =
    !serverReachable && !requireTypeDb

  private def requireTypeDb: Boolean =
    sys.env.get("TQLMATE_REQUIRE_TYPEDB").exists(v => v == "1" || v.equalsIgnoreCase("true")) ||
      sys.env.contains("CI")

  private def baseUrl: TypeDbUrl =
    val raw = sys.env
      .get("TYPEDB_URL")
      .orElse(sys.env.get("TQLMATE_TEST_URL"))
      .getOrElse("typedb://admin:password@127.0.0.1:1729/tqlmate_test")
    TypeDbUrl.parse(raw).toOption.get

  private def serverReachable: Boolean =
    val probe = RunnerOpts(
      url = baseUrl,
      migrationsDir = Path.of("."),
      schemaFile = Path.of("schema.tql"),
      strict = false,
      verbose = false,
      waitTimeout = None
    )
    val runner = Runner(probe)
    try runner.waitReady(2.seconds).isRight
    finally runner.close()

  test("migrate rollback and failed up"):
    if !serverReachable then
      if requireTypeDb then
        fail(s"TypeDB required but not reachable at ${baseUrl.address}")
      else
        println(s"skip: TypeDB not reachable at ${baseUrl.address}")
    else
      val millis = System.currentTimeMillis()
      val url = baseUrl.copy(database = s"tqlmate_$millis")
      val tmp = Files.createTempDirectory("tqlmate-it")
      val migrations = tmp.resolve("migrations")
      Files.createDirectories(migrations)

      Files.writeString(
        migrations.resolve("20240101000000_person.tql"),
        """|-- migrate:up
           |define
           |  entity person, owns name;
           |  attribute name, value string;
           |
           |-- migrate:down
           |undefine
           |  owns name from person;
           |  person;
           |  name;
           |""".stripMargin
      )
      Files.writeString(
        migrations.resolve("20240102000000_age.tql"),
        """|-- migrate:up
           |define
           |  entity person, owns age;
           |  attribute age, value integer;
           |
           |-- migrate:down
           |undefine
           |  owns age from person;
           |  age;
           |""".stripMargin
      )

      val opts = RunnerOpts(
        url = url,
        migrationsDir = migrations,
        schemaFile = tmp.resolve("schema.tql"),
        strict = false,
        verbose = true,
        waitTimeout = None
      )
      val runner = Runner(opts)
      try
        val _ = runner.drop()
        assert(runner.create().isRight, "create")
        assert(runner.migrate().isRight, "migrate")
        assertEquals(runner.status(quiet = true), Right(false))

        assert(runner.rollback().isRight, "rollback")
        assertEquals(runner.status(quiet = true), Right(true))

        assert(runner.dump().isRight, "dump")
        assert(runner.migrate().isRight, "re-migrate")

        Files.writeString(
          migrations.resolve("20240103000000_bad.tql"),
          "-- migrate:up\ndefine\n  this is not valid typeql !!!\n\n-- migrate:down\n\n"
        )
        assert(runner.migrate().isLeft, "bad migration should fail")
        assertEquals(
          runner.status(quiet = true),
          Right(true),
          "failed migration must not be recorded"
        )
        assert(runner.drop().isRight, "drop")
      finally runner.close()
