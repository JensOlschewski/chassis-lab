function [steps, time_array] = createTimeParam(t_start, t_end, stepsize)
    % createTimeParam Creates a time array and calculates the number of steps for a given time range and step size.
    %
    % This function takes in the start time, end time, and step size as inputs,
    % calculates the total number of steps required to move from the start time to
    % the end time with the specified step size, and creates a time array with
    % these time steps.
    %
    % Inputs:
    %   t_start - Start time of the time array.
    %   t_end   - End time of the time array.
    %   stepsize - Size of each step in the time array.
    %
    % Outputs:
    %   steps - Total number of steps calculated from start to end time.
    %   time_array - Array containing the time steps from start to end time.

    % Calculate the total number of steps
    steps = ((t_end - t_start) / stepsize) + 1;

    % Create the time array
    time_array = (t_start:stepsize:t_end)';
end
