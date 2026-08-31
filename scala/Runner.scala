package tqlmate

import java.nio.file.{Files, Path}
import java.time.{Instant, ZoneOffset}
import java.time.format.DateTimeFormatter
import java.time.temporal.ChronoUnit

import scala.concurrent.duration.*
import scala.util.Using

import com.typedb.driver.TypeDB
import com.typedb.driver.api.{Credentials, Driver, DriverOptions, DriverTlsConfig}

final case class RunnerOpts(
  url: TypeDbUrl,
  migrationsDir: Path,
  schemaFile: Path,
  strict: Boolean,
  verbose: Boolean,
  waitTimeout: Option[FiniteDuration]
)

final class Runner(opts: RunnerOpts):
  private var driverSlot: Option[Driver] = None

  private def driver(): Result[Driver] =
    driverSlot match
      case Some(d) => Right(d)
      case None =>
        val opened =
          opts.waitTimeout match
            case Some(t) => Runner.waitForServer(opts.url, t, opts.verbose).flatMap(_ => Runner.openDriver(opts.url))
            case None    => Runner.openDriver(opts.url)
        opened.map: d =>
          driverSlot = Some(d)
          d

  def close(): Unit =
    driverSlot.foreach(_.close())
    driverSlot = None

  def create(): Result[Unit] =
    val name = opts.url.database
    driver().flatMap: d =>
      Runner.edge:
        if d.databases().contains(name) then
          if opts.verbose then System.err.println(s"database already exists: $name")
        else
          d.databases().create(name)
          println(s"Created: $name")

  def drop(): Result[Unit] =
    val name = opts.url.database
    driver().flatMap: d =>
      Runner.edge:
        if !d.databases().contains(name) then
          if opts.verbose then System.err.println(s"database does not exist: $name")
        else
          d.databases().get(name).delete()
          println(s"Dropped: $name")

  def newMigration(name: String): Result[Path] =
    Runner.edge:
      Files.createDirectories(opts.migrationsDir)
      val path = Migration.newMigrationPath(opts.migrationsDir, name)
      Files.writeString(path, Migration.template)
      println(s"Created: $path")
      path

  def migrate(): Result[Unit] =
    val db = opts.url.database
    for
      d <- driver()
      exists <- Runner.edge(d.databases().contains(db))
      _ <-
        if !exists then Left(Error.msg(s"database does not exist: $db (run create or up)"))
        else Right(())
      _ <- Ledger.ensure(d, db)
      files <- Migration.listMigrationFiles(opts.migrationsDir)
      applied <- Ledger.appliedVersions(d, db)
      _ <- if opts.strict then Migration.checkStrictOrder(files, applied) else Right(())
      pending = files.filterNot(f => applied.contains(f.version))
      _ <-
        if pending.isEmpty then
          if opts.verbose then System.err.println("Migrations: nothing to apply")
          Right(())
        else
          pending.foldLeft[Result[Unit]](Right(())): (acc, m) =>
            acc.flatMap: _ =>
              Runner.applyUp(d, db, m.version, m.up, opts.verbose).map: _ =>
                println(s"Applied: ${m.label}")
    yield ()

  def rollback(): Result[Unit] =
    val db = opts.url.database
    for
      d <- driver()
      _ <- Ledger.ensure(d, db)
      files <- Migration.listMigrationFiles(opts.migrationsDir)
      applied <- Ledger.appliedVersions(d, db)
      _ <- applied.lastOption match
        case None =>
          if opts.verbose then System.err.println("Rollback: nothing to roll back")
          Right(())
        case Some(version) =>
          files.find(_.version == version) match
            case None =>
              Left(Error.msg(s"applied version ${version.value} has no matching migration file"))
            case Some(m) if m.down.isEmpty =>
              Left(Error.msg(s"migration ${m.label} has empty migrate:down"))
            case Some(m) =>
              Runner.applyDown(d, db, m.version, m.down, opts.verbose).map: _ =>
                println(s"Rolled back: ${m.label}")
    yield ()

  def status(quiet: Boolean): Result[Boolean] =
    val db = opts.url.database
    for
      d <- driver()
      exists <- Runner.edge(d.databases().contains(db))
      applied <-
        if exists then Ledger.appliedVersions(d, db).orElse(Right(Nil))
        else Right(Nil)
      files <- Migration.listMigrationFiles(opts.migrationsDir)
      _ <- if opts.strict then Migration.checkStrictOrder(files, applied) else Right(())
      rows = Migration.statusRows(files, applied)
      pending = rows.exists(_._2 == MigrationStatus.Pending)
      _ =
        if !quiet then
          if rows.isEmpty then println("No migrations found.")
          else
            rows.foreach: (m, st) =>
              val mark = st match
                case MigrationStatus.Applied => "[X]"
                case MigrationStatus.Pending => "[ ]"
              println(s"$mark ${m.label}")
    yield pending

  def dump(): Result[Unit] =
    val dbName = opts.url.database
    for
      d <- driver()
      schema <- Runner.edge(d.databases().get(dbName).schema())
      applied <- Ledger.appliedVersions(d, dbName).orElse(Right(Nil))
      files <- Migration.listMigrationFiles(opts.migrationsDir)
      _ <- Runner.edge:
        Option(opts.schemaFile.getParent).foreach(Files.createDirectories(_))
        val header = StringBuilder("-- Schema dumped by tqlmate\n-- Applied migrations:\n")
        if applied.isEmpty then header.append("--   (none)\n")
        else
          applied.foreach: v =>
            files.find(_.version == v) match
              case Some(m) => header.append(s"--   ${m.label}\n")
              case None    => header.append(s"--   ${v.value}\n")
        header.append('\n')
        header.append(schema.trim)
        header.append('\n')
        Files.writeString(opts.schemaFile, header.toString)
        println(s"Wrote: ${opts.schemaFile}")
    yield ()

  def load(): Result[Unit] =
    for
      text <- Runner.edge(Files.readString(opts.schemaFile))
      body = Runner.stripDumpHeader(text)
      _ <- if body.trim.isEmpty then Left(Error.msg("schema file is empty")) else Right(())
      d <- driver()
      db = opts.url.database
      _ <- Runner.edge:
        if !d.databases().contains(db) then d.databases().create(db)
      _ <- Ledger.schemaQueries(d, db, List(body))
      _ = println(s"Loaded: ${opts.schemaFile}")
    yield ()

  def waitReady(timeout: FiniteDuration): Result[Unit] =
    Runner.waitForServer(opts.url, timeout, opts.verbose)

  def up(): Result[Unit] =
    create().flatMap(_ => migrate())

object Runner:
  def resolveUrl(cliUrl: Option[String], dotenv: Map[String, String] = Map.empty): Result[TypeDbUrl] =
    def env(key: String): Option[String] =
      sys.env.get(key).orElse(dotenv.get(key))
    val raw =
      cliUrl
        .orElse(env("TYPEDB_URL"))
        .orElse(env("DATABASE_URL"))
        .getOrElse("typedb://admin:password@localhost:1729/typedb")
    TypeDbUrl.parse(raw)

  def defaultMigrationsDir: Path =
    Path.of(sys.env.getOrElse("TQLMATE_MIGRATIONS_DIR", "db/migrations"))

  def defaultSchemaFile: Path =
    Path.of(sys.env.getOrElse("TQLMATE_SCHEMA_FILE", "db/schema.tql"))

  private[tqlmate] def openDriver(url: TypeDbUrl): Result[Driver] =
    edge:
      val tls =
        if url.tls then DriverTlsConfig.enabledWithNativeRootCA()
        else DriverTlsConfig.disabled()
      TypeDB.driver(url.address, Credentials(url.username, url.password), DriverOptions(tls))

  private[tqlmate] def waitForServer(
    url: TypeDbUrl,
    timeout: FiniteDuration,
    verbose: Boolean
  ): Result[Unit] =
    val deadline = Instant.now().plusMillis(timeout.toMillis)
    def loop(): Result[Unit] =
      openDriver(url) match
        case Right(d) =>
          d.close()
          if verbose then System.err.println(s"TypeDB available at ${url.address}")
          Right(())
        case Left(e) =>
          if Instant.now().isAfter(deadline) then
            Left(Error.msg(s"timed out waiting for TypeDB at ${url.address}: ${e.message}"))
          else
            if verbose then System.err.println(s"waiting for TypeDB at ${url.address} (${e.message})")
            Thread.sleep(500)
            loop()
    loop()

  private[tqlmate] def applyUp(
    driver: Driver,
    database: String,
    version: Version,
    up: String,
    verbose: Boolean
  ): Result[Unit] =
    if up.trim.isEmpty then Left(Error.msg(s"migration ${version.value} has empty migrate:up"))
    else
      val appliedAt = DateTimeFormatter
        .ofPattern("yyyy-MM-dd'T'HH:mm:ss")
        .withZone(ZoneOffset.UTC)
        .format(Instant.now().truncatedTo(ChronoUnit.SECONDS))
      val insert = Ledger.recordInsert(version, appliedAt)
      if verbose then System.err.println(s"-> up ${version.value}")
      Ledger.schemaQueries(driver, database, List(up, insert))

  private[tqlmate] def applyDown(
    driver: Driver,
    database: String,
    version: Version,
    down: String,
    verbose: Boolean
  ): Result[Unit] =
    val delete = Ledger.recordDelete(version)
    if verbose then System.err.println(s"-> down ${version.value}")
    Ledger.schemaQueries(driver, database, List(down, delete))

  private[tqlmate] def stripDumpHeader(text: String): String =
    text.linesIterator
      .dropWhile: l =>
        val t = l.trim
        t.isEmpty || t.startsWith("--")
      .mkString("\n")

  private[tqlmate] def edge[A](body: => A): Result[A] =
    try Right(body)
    catch case e: Exception => Left(Error.fromThrowable(e))

  extension [A](r: Result[A])
    private[tqlmate] def orElse(fallback: Result[A]): Result[A] =
      r match
        case Left(_)  => fallback
        case Right(a) => Right(a)
