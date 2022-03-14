from datetime import datetime

from flask import Flask, render_template, request
import flask
from flask_login import LoginManager, login_required, login_user, current_user, logout_user
from flask_wtf import FlaskForm
from wtforms import Field, StringField, PasswordField, Label, HiddenField
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
    username = StringField('username', validators=[DataRequired()])
    password = PasswordField('Password', validators=[DataRequired()])


class CommaListField(Field):
    widget = TextInput()

    def _value(self):
        if self.data:
            return u', '.join(self.data)
        else:
            return u''

    def process_formdata(self, valuelist):
        if valuelist and valuelist[0]:
            self.data = [x.strip() for x in valuelist[0].split(',')]
        else:
            self.data = []  


class AddProjectForm(FlaskForm):
    name = StringField('name', validators=[DataRequired()])
    members = CommaListField('members', validators=[])


class ChangeProjectMembersForm(FlaskForm):
    members = CommaListField('members', validators=[DataRequired()])


def default_dataset_name():
    now = datetime.now()
    return now.strftime('%Y-%m-%d ')


class CreateDatasetForm(FlaskForm):
    name = StringField('name', validators=[DataRequired()])


class SealDatasetForm(FlaskForm):
    pass


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
    # contains: project list, actions: create new project, links to project details
    createprojectform = AddProjectForm()
    return render_template(
        'index.html.j2',
        projects=projects['projects'],
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
    else:
        return render_template('login.html.j2', form=form)


@app.route("/add/", methods=["GET", "POST"])
@login_required
def add_project():
    form = AddProjectForm()
    if request.method == 'POST' and form.validate():
        result = client.create_project(
            name=form.name.data,
            owner=current_user.user_id,
            members=form.members.data
        )

        if result['errno'] == 0:
            flask.flash(f"Project {form.name.data} created.")
            return flask.redirect(flask.url_for('index'))
        else:
            flask.flash(f"Error {result['message']}")
            return flask.redirect(
                flask.url_for(
                    'add_project',
                    name=form.name.data,
                    member=form.members.data
                )
            )
    args = request.args.to_dict(flat=False)
    members = args.get('member', [])
    name = args.get('name', [''])
    form.name.data = name[0]
    form.members.data = members
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


@app.route("/<name>/addmembers/", methods=["GET", "POST"])
@login_required
def add_project_members(name: str):
    # First perform member action
    form = ChangeProjectMembersForm()
    if request.method == 'POST' and form.validate():
        result = client.change_project_members(
            projectname=name,
            op='add',
            members=form.members.data,
            user=current_user.user_id,
        )
        if result['errno'] == 0:
            flask.flash(f"Members {form.members.data} added.")
            return flask.redirect(flask.url_for('project', name=name))
        else:
            flask.flash(f"Error {result['message']}.")
            return flask.redirect(flask.url_for('add_project_members', name=name))

    pr_info = client.projectdetails(name)
    if pr_info['errno'] == 0:
        return render_template(
            'addprojectmembers.html.j2',
            form=form,
            project=pr_info['project']
        )
    else:
        # FIXME a 403, 404 or other error codes
        # may be appropriate as well -- to be refined
        return render_template(
            'error.html.j2',
            result=pr_info
        )


@app.route("/<name>/removemembers/", methods=["GET", "POST"])
@login_required
def remove_project_members(name: str):
    args = request.args.to_dict(flat=False)
    remove_me = args.get('member', [])
    # First perform member action
    form = ChangeProjectMembersForm()
    if request.method == 'POST' and form.validate():
        result = client.change_project_members(
            projectname=name,
            op='remove',
            members=form.members.data,
            user=current_user.user_id,
        )
        if result['errno'] == 0:
            flask.flash(f"Members {form.members.data} removed.")
            return flask.redirect(flask.url_for('project', name=name))
        else:
            flask.flash(f"Error {result['message']}.")
            return flask.redirect(flask.url_for('remove_project_members', name=name))
    form.members.data = remove_me
    pr_info = client.projectdetails(name)
    if pr_info['errno'] == 0:
        return render_template(
            'removeprojectmembers.html.j2',
            form=form,
            project=pr_info['project']
        )
    else:
        # FIXME a 403, 404 or other error codes
        # may be appropriate as well -- to be refined
        return render_template(
            'error.html.j2',
            result=pr_info
        )


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
            flask.flash(f'Dataset "{form.name.data}" created in project "{name}".')
            return flask.redirect(flask.url_for('project', name=name))
        else:
            flask.flash(f"Error: {result['message']}.")
            return flask.redirect(
                flask.url_for(
                    'create_dataset',
                    name=name,
                    dsname=form.name.data,
                )
            )
    form.name.data = request.args.get('dsname', default=default_dataset_name())
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
            flask.flash(f'Dataset "{dataset}" in project "{name}" sealed.')
            return flask.redirect(flask.url_for('project', name=name))
        else:
            flask.flash(f"Error: {result['message']}.")
            return flask.redirect(
                flask.url_for(
                    'seal_dataset',
                    name=name,
                    dataset=dataset,
                )
            )
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
