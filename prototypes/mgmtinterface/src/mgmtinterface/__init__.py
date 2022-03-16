from datetime import datetime
from pydoc import cli
import time

from flask import Flask, render_template, request
import flask
from flask_login import LoginManager, login_required, login_user, current_user, logout_user
from flask_wtf import FlaskForm
from wtforms import Field, StringField, PasswordField, SelectMultipleField
from wtforms.widgets import TextInput
from wtforms.validators import DataRequired
from urllib.parse import urlparse, urljoin

from .permissionsclient import PermissionsClient


def is_safe_url(target):
    ref_url = urlparse(request.host_url)
    test_url = urlparse(urljoin(request.host_url, target))
    return test_url.scheme in ('http', 'https') and ref_url.netloc == test_url.netloc


app = Flask(__name__)
login_manager = LoginManager()
login_manager.init_app(app)

app.secret_key = b'caHudnec[drav2'

client = PermissionsClient("http://localhost:5050")


class User:
    def __init__(self, user_id):
        self.user_id = user_id

    @property
    def is_authenticated(self):
        return True

    @property
    def is_active(self):
        return True

    @property
    def is_anonymous(self):
        return False

    def get_id(self):
        return self.user_id


class LoginForm(FlaskForm):
    username = StringField(
        'Username',
        validators=[DataRequired()],
        render_kw={'autofocus': True}
    )
    password = PasswordField('Password', validators=[DataRequired()])


class AddProjectForm(FlaskForm):
    name = StringField(
        'name', validators=[DataRequired()],
        render_kw={'autofocus': True}
    )
    members = SelectMultipleField(
        'members',
        validators=[]
    )


class ChangeProjectMembersForm(FlaskForm):
    members = SelectMultipleField(
        'members',
        validators=[]
    )


def default_dataset_name():
    now = datetime.now()
    return now.strftime('%Y-%m-%d ')


class CreateDatasetForm(FlaskForm):
    name = StringField(
        'name', validators=[DataRequired()],
        render_kw={'autofocus': True}
    )


class SealDatasetForm(FlaskForm):
    pass


@app.template_filter()
def format_unixtime(value):
    d = datetime.fromtimestamp(value)
    return d.strftime('%Y-%m-%d %H:%M')


@login_manager.user_loader
def load_user(user_id):
    return User(user_id=user_id)


@login_manager.unauthorized_handler
def unauthorized_callback():
    flask.flash("Unauthorized, login required.")
    return flask.redirect(flask.url_for('login'))


@app.route("/", methods=["GET"])
@login_required
def index():
    projects = client.list_projects()
    plist = sorted(projects['projects'], key=lambda p: p['mtime'], reverse=True)
    ownprojects = []
    memberprojects = []
    otherprojects = []
    uid = current_user.user_id
    for p in plist:
        if p['owner'] == uid:
            ownprojects.append(p)
        elif uid in p['members']:
            memberprojects.append(p)
        else:
            otherprojects.append(p)

    # contains: project list, actions: create new project, links to project details
    createprojectform = AddProjectForm()
    return render_template(
        'index.html.j2',
        ownprojects=ownprojects,
        memberprojects=memberprojects,
        otherprojects=otherprojects,
        createprojectform=createprojectform,
    )


@app.route("/login/", methods=["GET", "POST"])
def login():
    form = LoginForm()
    if request.method == 'POST' and form.validate():
        auth_result = client.authenticate(form.username.data, form.password.data)

        if auth_result['errno'] == 0:
            login_user(User(user_id=form.username.data))
            next_url = flask.request.args.get('next')
            if not is_safe_url(next_url):
                return flask.abort(400)
            return flask.redirect(next_url or flask.url_for('index'))
        else:
            flask.flash(f"Error: {auth_result['message']}")
    return render_template('login.html.j2', form=form)


@app.route("/add/", methods=["GET", "POST"])
@login_required
def add_project():
    form = AddProjectForm()
    allusers = client.list_users()
    if allusers['errno'] != 0:
        return render_template(
            'error.html.j2',
            result=allusers,
        )
    form.members.choices = [
        (name, name) for name in allusers['users']
    ]
    if request.method == 'POST' and form.validate():
        form_set = set(form.members.data)
        invalids = form_set - set(allusers['users'])
        if invalids:
            # Users that are not known users of the data management
            flask.flash(f"Bad request, invalid options")
            return render_template(
                'error.html.j2',
                result=allusers,
            )
        result = client.create_project(
            name=form.name.data,
            owner=current_user.user_id,
            members=form.members.data
        )
        if result['errno'] == 0:
            flask.flash(f"Project {form.name.data} created.")
            return flask.redirect(flask.url_for('project', name=form.name.data))
        else:
            flask.flash(f"Error {result['message']}")
    return render_template(
        'addproject.html.j2',
        form=form,
    )


@app.route("/<name>/details/", methods=["GET"])
@login_required
def project(name: str):
    pr_info = client.projectdetails(name)
    if pr_info['errno'] == 0:
        return render_template(
            'projectdetails.html.j2',
            project=pr_info['project'],
            datasets=pr_info['datasets'],
        )
    else:
        return render_template(
            'error.html.j2',
            result=pr_info
        )


@app.route("/<name>/members/", methods=["GET", "POST"])
@login_required
def project_members(name: str):
    def error(info):
        # FIXME a 403, 404 or other error codes
        # may be appropriate as well -- to be refined
        return render_template(
            'error.html.j2',
            result=info
        )
    is_error = False
    # First perform member action
    form = ChangeProjectMembersForm()
    pr_info = client.projectdetails(name)
    
    if pr_info['errno'] == 0:
        allusers = client.list_users()
        if allusers['errno'] != 0:
            return error(allusers)
        form.members.choices = [
            (name, name) for name in allusers['users']
        ]
        updated = False
        if request.method == 'POST' and form.validate():
            updated = True
            form_set = set(form.members.data)
            project_set = set(pr_info['project']['members'])
            invalids = form_set - set(allusers['users'])
            if invalids:
                # Users that are not known users of the data management
                flask.flash(f"Bad request, invalid options")
                is_error = True
            to_add = form_set - project_set
            to_remove = project_set - form_set
            if to_add and not is_error:
                result = client.change_project_members(
                    projectname=name,
                    op='add',
                    members=list(to_add),
                    user=current_user.user_id,
                )
                if result['errno'] != 0:
                    flask.flash(f"Error adding members {to_add}: {result['message']}.")
                    is_error = True
            if to_remove and not is_error:
                result = client.change_project_members(
                    projectname=name,
                    op='remove',
                    members=list(to_remove),
                    user=current_user.user_id,
                )
                if result['errno'] != 0:
                    flask.flash(f"Error removing members {to_remove}: {result['message']}.")
                    is_error = True
            if not is_error:
                if to_add or to_remove:
                    flask.flash("Project members updated.")
                return flask.redirect(flask.url_for('project', name=name))
        if updated:
            pr_info = client.projectdetails(name)
        if pr_info['errno'] == 0:
            form.members.data = pr_info['project']['members']
            return render_template(
                'projectmembers.html.j2',
                form=form,
                project=pr_info['project']
            )
    return(error(pr_info))


@app.route("/<name>/createdataset/", methods=["GET", "POST"])
@login_required
def create_dataset(name: str):
    # First perform member action
    form = CreateDatasetForm()
    if request.method == 'POST' and form.validate():
        result = client.create_dataset(
            projectname=name,
            name=form.name.data,
            user=current_user.user_id,
        )
        if result['errno'] == 0:
            flask.flash(f'Session "{form.name.data}" created in project "{name}".')
            return flask.redirect(flask.url_for('project', name=name))
        else:
            flask.flash(f"Error: {result['message']}.")
    else:
        form.name.data = default_dataset_name()
    pr_info = client.projectdetails(name)
    if pr_info['errno'] == 0:
        return render_template(
            'createdataset.html.j2',
            project=pr_info['project'],
            datasets=pr_info['datasets'],
            form=form
        )
    else:
        # FIXME a 403, 404 or other error codes
        # may be appropriate as well -- to be refined
        return render_template(
            'error.html.j2',
            result=pr_info
        )


@app.route("/<name>/seal/<dataset>/", methods=["GET", "POST"])
@login_required
def seal_dataset(name: str, dataset: str):
    # First perform member action
    form = SealDatasetForm()
    if request.method == 'POST' and form.validate():
        result = client.seal_dataset(
            projectname=name,
            name=dataset,
            user=current_user.user_id,
        )
        if result['errno'] == 0:
            flask.flash(f'Session "{dataset}" in project "{name}" sealed.')
            return flask.redirect(flask.url_for('project', name=name))
        else:
            flask.flash(f"Error: {result['message']}.")

    pr_info = client.projectdetails(name)
    if pr_info['errno'] == 0:
        return render_template(
            'sealdataset.html.j2',
            project=pr_info['project'],
            datasets=pr_info['datasets'],
            form=form,
            dsname=dataset,
        )
    else:
        # FIXME a 403, 404 or other error codes
        # may be appropriate as well -- to be refined
        return render_template(
            'error.html.j2',
            result=pr_info
        )


@app.route('/logout')
@login_required
def logout():
    logout_user()
    flask.flash('You were logged out.')
    return flask.redirect(flask.url_for('login'))
