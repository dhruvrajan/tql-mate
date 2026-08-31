package tqlmate

import java.nio.file.{Files, Path}
import scala.concurrent.duration.*
import scala.jdk.CollectionConverters.*

import cats.syntax.all.*
import com.monovore.decline.*

enum Cmd:
  case New(name: String)
  case Up, Create, Drop, Migrate, Rollback, Dump, Load
  case Status(exitCode: Boolean, quiet: Boolean)
  case Wait(timeoutSecs: Long)

final case class GlobalOpts(
  url: Option[String],
  envPairs: List[String],
  envFile: Path,
  migrationsDir: Option[Path],
  schemaFile: Option[Path],
  waitSecs: Option[Long],
  strict: Boolean,
  verbose: Boolean
)

final case class CliArgs(global: GlobalOpts, cmd: Cmd)

object Cli:
  private val global: Opts[GlobalOpts] =
    (
      Opts.option[String]("url", "TypeDB URL", short = "u").orNone,
      Opts.options[String]("env", "Set KEY=VALUE", short = "e").orEmpty,
      Opts.option[Path]("env-file", "Env file").withDefault(Path.of(".env")),
      Opts.option[Path]("migrations-dir", "Migrations directory", short = "d").orNone,
      Opts.option[Path]("schema-file", "Schema dump path").orNone,
      Opts.option[Long]("wait", "Seconds to wait for TypeDB before connecting").orNone,
      Opts.flag("strict", "Fail on out-of-order pending migrations").orFalse,
      Opts.flag("verbose", "Verbose logging", short = "v").orFalse
    ).mapN(GlobalOpts.apply)

  private val newCmd =
    Opts.subcommand("new", "Create a new migration file"):
      Opts.argument[String]("name").map(Cmd.New.apply)

  private val upCmd =
    Opts.subcommand("up", "Create database and migrate")(Opts(Cmd.Up))
  private val createCmd =
    Opts.subcommand("create", "Create the database")(Opts(Cmd.Create))
  private val dropCmd =
    Opts.subcommand("drop", "Drop the database")(Opts(Cmd.Drop))
  private val migrateCmd =
    Opts.subcommand("migrate", "Apply pending migrations")(Opts(Cmd.Migrate))
  private val rollbackCmd =
    Opts.subcommand("rollback", "Roll back the latest migration")(Opts(Cmd.Rollback))
  private val downCmd =
    Opts.subcommand("down", "Alias for rollback")(Opts(Cmd.Rollback))
  private val statusCmd =
    Opts.subcommand("status", "Show migration status"):
      (
        Opts.flag("exit-code", "Exit 1 when migrations are pending").orFalse,
        Opts.flag("quiet", "Suppress status output").orFalse
      ).mapN(Cmd.Status.apply)
  private val dumpCmd =
    Opts.subcommand("dump", "Write live schema to schema file")(Opts(Cmd.Dump))
  private val loadCmd =
    Opts.subcommand("load", "Load schema file into the database")(Opts(Cmd.Load))
  private val waitCmd =
    Opts.subcommand("wait", "Wait until TypeDB accepts connections"):
      Opts.option[Long]("timeout", "Timeout in seconds").withDefault(60L).map(Cmd.Wait.apply)

  private val command: Opts[Cmd] =
    newCmd
      .orElse(upCmd)
      .orElse(createCmd)
      .orElse(dropCmd)
      .orElse(migrateCmd)
      .orElse(rollbackCmd)
      .orElse(downCmd)
      .orElse(statusCmd)
      .orElse(dumpCmd)
      .orElse(loadCmd)
      .orElse(waitCmd)

  val opts: Opts[CliArgs] = (global, command).mapN(CliArgs.apply)

  val app: Command[CliArgs] =
    Command("tqlmate-scala", "TypeDB 3.x migration tool (Scala)"):
      opts

  def parse(argv: List[String]): Either[Help, CliArgs] = app.parse(argv)

object Main:
  def main(args: Array[String]): Unit =
    Cli.parse(args.toList) match
      case Left(help) =>
        System.err.println(help)
        sys.exit(if help.errors.nonEmpty then 1 else 0)
      case Right(cli) =>
        val dotenv = readEnvFile(cli.global.envFile) ++ parseEnvPairs(cli.global.envPairs)
        run(cli, dotenv) match
          case Left(err) =>
            System.err.println(s"Error: ${err.message}")
            sys.exit(1)
          case Right(code) =>
            sys.exit(code)

  private def run(cli: CliArgs, dotenv: Map[String, String]): Result[Int] =
    for
      url <- Runner.resolveUrl(cli.global.url, dotenv)
      runnerOpts = RunnerOpts(
        url = url,
        migrationsDir = cli.global.migrationsDir.getOrElse(Runner.defaultMigrationsDir),
        schemaFile = cli.global.schemaFile.getOrElse(Runner.defaultSchemaFile),
        strict = cli.global.strict || truthy(dotenv.get("TQLMATE_STRICT").orElse(sys.env.get("TQLMATE_STRICT"))),
        verbose = cli.global.verbose,
        waitTimeout = cli.global.waitSecs.map(_.seconds)
      )
      code <-
        val runner = Runner(runnerOpts)
        try
          cli.cmd match
            case Cmd.New(name)              => runner.newMigration(name).as(0)
            case Cmd.Up                     => runner.up().as(0)
            case Cmd.Create                 => runner.create().as(0)
            case Cmd.Drop                   => runner.drop().as(0)
            case Cmd.Migrate                => runner.migrate().as(0)
            case Cmd.Rollback               => runner.rollback().as(0)
            case Cmd.Status(exitCode, quiet) =>
              runner.status(quiet).map(pending => if exitCode && pending then 1 else 0)
            case Cmd.Dump                   => runner.dump().as(0)
            case Cmd.Load                   => runner.load().as(0)
            case Cmd.Wait(timeoutSecs) =>
              val secs = cli.global.waitSecs.getOrElse(timeoutSecs)
              runner.waitReady(secs.seconds).as(0)
        finally runner.close()
    yield code

  private def readEnvFile(path: Path): Map[String, String] =
    if !Files.isRegularFile(path) then Map.empty
    else
      Files
        .readAllLines(path)
        .asScala
        .map(_.trim)
        .filter(l => l.nonEmpty && !l.startsWith("#"))
        .flatMap:
          case s"$k=$v" => Some(k -> stripQuotes(v))
          case _        => None
        .toMap

  private def parseEnvPairs(pairs: List[String]): Map[String, String] =
    pairs.flatMap:
      case s"$k=$v" => Some(k -> v)
      case _        => None
    .toMap

  private def stripQuotes(v: String): String =
    if (v.startsWith("\"") && v.endsWith("\"")) || (v.startsWith("'") && v.endsWith("'")) then
      v.substring(1, v.length - 1)
    else v

  private def truthy(v: Option[String]): Boolean =
    v.exists(s => s == "1" || s.equalsIgnoreCase("true") || s.equalsIgnoreCase("yes"))

  extension [A](r: Result[A])
    private def as(code: Int): Result[Int] = r.map(_ => code)
